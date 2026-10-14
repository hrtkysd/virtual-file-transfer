use super::OleContext;
use super::data_object::DataObject;
use super::error::from_windows_error;

use crate::cancellation::CancellationToken;
use crate::{PlatformErrorKind, Result, TransferRequest};

use std::ffi::c_void;
use windows::Win32::Foundation::S_OK;
use windows::Win32::System::Com::IDataObject;
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
        let hr = unsafe { ole_is_current_clipboard(self.data_object.as_raw()) };

        if hr == S_OK {
            let _ = unsafe { OleSetClipboard(None::<&IDataObject>) };
        }
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
}
