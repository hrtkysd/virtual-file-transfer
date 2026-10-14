use super::OleContext;
use super::data_object::DataObject;
use super::error::from_windows_error;

use crate::cancellation::CancellationToken;
use crate::{PlatformErrorKind, Result, TransferRequest};

use std::ffi::c_void;
use windows::Win32::Foundation::S_OK;
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard};
use windows::Win32::System::Ole::OleSetClipboard;
use windows::core::{HRESULT, Interface};

#[link(name = "ole32")]
unsafe extern "system" {
    #[link_name = "OleIsCurrentClipboard"]
    fn ole_is_current_clipboard(data_object: *mut c_void) -> HRESULT;
}

pub(crate) struct ClipboardSession {
    data_object: IDataObject,
    cancellation: CancellationToken,
    _ole: OleContext,
}

impl ClipboardSession {
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}

impl Drop for ClipboardSession {
    fn drop(&mut self) {
        // Keep the clipboard locked while checking ownership and clearing it,
        // so another application cannot replace its contents between these operations.
        if unsafe { OpenClipboard(None) }.is_err() {
            return;
        }

        let is_current = unsafe { ole_is_current_clipboard(self.data_object.as_raw()) == S_OK };

        if is_current {
            let _ = unsafe { EmptyClipboard() };
        }

        let _ = unsafe { CloseClipboard() };
    }
}

pub(crate) fn copy_to_clipboard(request: TransferRequest) -> Result<ClipboardSession> {
    let ole = OleContext::new()
        .map_err(|error| from_windows_error(PlatformErrorKind::InitializationFailed, error))?;

    let TransferRequest {
        files,
        cancellation: token,
    } = request;

    let data_object: IDataObject = DataObject::new(files, token.clone(), None)
        .map_err(|error| from_windows_error(PlatformErrorKind::TransferFailed, error))?
        .into();

    // Unlike drag-and-drop, OleSetClipboard only registers the data object.
    // File contents are retrieved on demand when pasted, so asynchronous mode is not required for clipboard registration.
    unsafe { OleSetClipboard(&data_object) }
        .map_err(|error| from_windows_error(PlatformErrorKind::TransferFailed, error))?;

    Ok(ClipboardSession {
        data_object,
        cancellation: token,
        _ole: ole,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{FileSource, SourceReader, VirtualFile};

    use std::io::{self, Cursor};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct TrackedSource {
        drops: Arc<AtomicUsize>,
    }

    impl FileSource for TrackedSource {
        fn open(&self) -> io::Result<SourceReader> {
            Ok(SourceReader::seekable(Cursor::new(b"hello".to_vec())))
        }
    }

    impl Drop for TrackedSource {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn make_request(drops: Arc<AtomicUsize>) -> TransferRequest {
        let source = TrackedSource { drops };
        let file = VirtualFile::new("hello.txt", Box::new(source)).unwrap();

        TransferRequest::new(vec![file])
    }

    fn clear_clipboard() {
        unsafe {
            OleSetClipboard(None::<&IDataObject>).unwrap();
        }
    }

    #[test]
    #[ignore = "Uses the system clipboard"]
    fn replacing_clipboard_releases_previous_data_object() {
        let first_drops = Arc::new(AtomicUsize::new(0));
        let second_drops = Arc::new(AtomicUsize::new(0));

        let first = copy_to_clipboard(make_request(Arc::clone(&first_drops))).unwrap();

        assert_eq!(first_drops.load(Ordering::SeqCst), 0);

        let second = copy_to_clipboard(make_request(Arc::clone(&second_drops))).unwrap();

        // The first session still holds a reference to its data object.
        assert_eq!(first_drops.load(Ordering::SeqCst), 0);

        // Dropping the old session releases its data object
        // without clearing the new clipboard contents.
        drop(first);
        assert_eq!(first_drops.load(Ordering::SeqCst), 1);
        assert_eq!(second_drops.load(Ordering::SeqCst), 0);

        // The second session still holds a reference after clearing.
        clear_clipboard();
        assert_eq!(second_drops.load(Ordering::SeqCst), 0);

        drop(second);
        assert_eq!(second_drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    #[ignore = "Uses the system clipboard"]
    fn dropping_session_releases_current_clipboard_data_object() {
        let drops = Arc::new(AtomicUsize::new(0));

        let session = copy_to_clipboard(make_request(Arc::clone(&drops))).unwrap();

        assert_eq!(drops.load(Ordering::SeqCst), 0);

        // Dropping the session should clear its clipboard data
        // before OLE is uninitialized.
        drop(session);

        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    #[ignore = "Uses the system clipboard"]
    fn dropping_old_session_preserves_new_clipboard_data() {
        let first_drops = Arc::new(AtomicUsize::new(0));
        let second_drops = Arc::new(AtomicUsize::new(0));

        let first = copy_to_clipboard(make_request(Arc::clone(&first_drops))).unwrap();

        let second = copy_to_clipboard(make_request(Arc::clone(&second_drops))).unwrap();

        // The first session no longer owns the clipboard.
        drop(first);

        assert_eq!(second_drops.load(Ordering::SeqCst), 0);

        // The second data object should still be current.
        let hr = unsafe { ole_is_current_clipboard(second.data_object.as_raw()) };
        assert_eq!(hr, S_OK);

        drop(second);

        assert_eq!(first_drops.load(Ordering::SeqCst), 1);
        assert_eq!(second_drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    #[ignore = "Clipboard lock helper"]
    fn hold_clipboard_in_subprocess() {
        if std::env::var_os("VFT_CLIPBOARD_HOLDER").is_none() {
            return;
        }

        use std::io::{Read, Write};
        use windows::Win32::System::DataExchange::{CloseClipboard, OpenClipboard};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPED,
        };
        use windows::core::w;

        // Create a window owned by this process and thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Clipboard Test Holder"),
                WS_OVERLAPPED,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        };

        // OpenClipboard(NULL) may succeed even when subsequent clipboard operations fail with ERROR_CLIPBOARD_NOT_OPEN.
        // Use an explicit HWND to avoid this observed behavior.
        // See: https://cygwin.com/pipermail/cygwin/2026-February/259452.html
        unsafe { OpenClipboard(Some(hwnd)).unwrap() };

        println!("VFT_LOCKED");
        std::io::stdout().flush().unwrap();

        // Wait until the parent has dropped ClipboardSession.
        let mut signal = [0u8; 1];
        let read_result = std::io::stdin().read_exact(&mut signal);

        let close_result = unsafe { CloseClipboard() };
        unsafe { DestroyWindow(hwnd).unwrap() };

        read_result.unwrap();
        close_result.unwrap();
    }

    #[test]
    #[ignore = "Uses the system clipboard"]
    fn dropping_session_while_clipboard_is_locked_does_not_block() {
        use std::io::{BufRead, BufReader, Write};
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        let drops = Arc::new(AtomicUsize::new(0));
        let session = copy_to_clipboard(make_request(Arc::clone(&drops))).unwrap();

        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "windows::clipboard::tests::hold_clipboard_in_subprocess",
                "--ignored",
                "--nocapture",
            ])
            .env("VFT_CLIPBOARD_HOLDER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).unwrap(), 0);
            if line.contains("VFT_LOCKED") {
                break;
            }
        }

        // The child process now holds the clipboard.
        let start = Instant::now();
        drop(session);
        let elapsed = start.elapsed();

        child.stdin.take().unwrap().write_all(b"x").unwrap();
        assert!(child.wait().unwrap().success());

        assert!(
            elapsed < Duration::from_secs(3),
            "ClipboardSession::drop() blocked for {elapsed:?}"
        );

        // Restore an accessible, empty clipboard.
        let _ole = OleContext::new().unwrap();
        clear_clipboard();
    }
}
