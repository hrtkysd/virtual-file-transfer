use std::{
    cell::RefCell,
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use virtual_file_transfer::{
    ClipboardSession, FactorySource, SourceReader, TransferRequest, VirtualFile, copy_to_clipboard,
};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT, TextOutW};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetMessageW, IDC_ARROW, KillTimer, LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SetTimer,
    TranslateMessage, WINDOW_EX_STYLE, WM_DESTROY, WM_LBUTTONDOWN, WM_PAINT, WM_RBUTTONDOWN,
    WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::{Error, Result, w};

const FILE_SIZE: u64 = 2 * 1024 * 1024; // 2 MiB per file
const CHUNK_SIZE: usize = 64 * 1024; // 64 KiB per read
const READ_DELAY: Duration = Duration::from_millis(120);
const TIMER_ID: usize = 1;

#[derive(Default)]
struct ReadStats {
    opens: AtomicU64,
    reads: AtomicU64,
    bytes: AtomicU64,
    active_readers: AtomicU64,
}

struct SlowReader {
    name: &'static str,
    fill: u8,
    remaining: u64,
    stats: Arc<ReadStats>,
}

impl Read for SlowReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = buffer.len().min(CHUNK_SIZE).min(self.remaining as usize);
        if count == 0 {
            return Ok(0);
        }

        thread::sleep(READ_DELAY);
        buffer[..count].fill(self.fill);
        self.remaining -= count as u64;
        self.stats.reads.fetch_add(1, Ordering::Relaxed);
        self.stats.bytes.fetch_add(count as u64, Ordering::Relaxed);

        eprintln!(
            "[Clipboard/Read] {} thread={:?} bytes={} remaining={}",
            self.name,
            thread::current().id(),
            count,
            self.remaining,
        );
        Ok(count)
    }
}

impl Drop for SlowReader {
    fn drop(&mut self) {
        self.stats.active_readers.fetch_sub(1, Ordering::Relaxed);
        eprintln!(
            "[Clipboard/Reader Drop] {} thread={:?}",
            self.name,
            thread::current().id(),
        );
    }
}

fn make_file(
    name: &'static str,
    fill: u8,
    stats: &Arc<ReadStats>,
) -> virtual_file_transfer::Result<VirtualFile> {
    let stats = Arc::clone(stats);
    let source = FactorySource::new(move || {
        stats.opens.fetch_add(1, Ordering::Relaxed);
        stats.active_readers.fetch_add(1, Ordering::Relaxed);
        Ok(SourceReader::streaming(SlowReader {
            name,
            fill,
            remaining: FILE_SIZE,
            stats: Arc::clone(&stats),
        }))
    });

    Ok(VirtualFile::new(name, Box::new(source))?.with_size(FILE_SIZE))
}

fn publish_clipboard() -> virtual_file_transfer::Result<(ClipboardSession, Arc<ReadStats>)> {
    let stats = Arc::new(ReadStats::default());
    let files = vec![
        make_file("slow-clipboard-a.bin", b'A', &stats)?,
        make_file("slow-clipboard-b.bin", b'B', &stats)?,
    ];

    let session = copy_to_clipboard(TransferRequest::from(files))?;
    Ok((session, stats))
}

#[derive(Default)]
struct AppState {
    clipboard: Option<ClipboardSession>,
    stats: Option<Arc<ReadStats>>,
    ticks: u64,
    right_clicks: u64,
    error: Option<String>,
}

thread_local! {
    static STATE: RefCell<AppState> = RefCell::new(AppState::default());
}

fn refresh(hwnd: HWND) {
    unsafe { _ = InvalidateRect(Some(hwnd), None, true) };
}

fn paint(hwnd: HWND) {
    let lines = STATE.with(|state| {
        let state = state.borrow();
        let (opens, reads, bytes, active) = state.stats.as_ref().map_or((0, 0, 0, 0), |stats| {
            (
                stats.opens.load(Ordering::Relaxed),
                stats.reads.load(Ordering::Relaxed),
                stats.bytes.load(Ordering::Relaxed),
                stats.active_readers.load(Ordering::Relaxed),
            )
        });

        [
            "Left-click ONCE to copy two virtual files.".to_owned(),
            "Switch to Explorer and press Ctrl+V (try twice).".to_owned(),
            "Keep this window open during the paste.".to_owned(),
            "Right-click here during the transfer to test responsiveness.".to_owned(),
            format!("Clipboard set: {}", state.clipboard.is_some()),
            format!("Heartbeat (100 ms): {}", state.ticks),
            format!("Right clicks: {}", state.right_clicks),
            format!("Reader opens: {opens}, active: {active}"),
            format!("Read calls: {reads}, bytes served: {bytes}"),
            format!("Last error: {}", state.error.as_deref().unwrap_or("None")),
        ]
    });

    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    for (index, line) in lines.iter().enumerate() {
        let wide: Vec<u16> = line.encode_utf16().collect();
        unsafe { _ = TextOutW(hdc, 16, 18 + index as i32 * 30, &wide) };
    }
    unsafe { _ = EndPaint(hwnd, &ps) };
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_LBUTTONDOWN => {
            let already_set = STATE.with(|state| state.borrow().clipboard.is_some());
            if !already_set {
                let result = publish_clipboard();
                STATE.with(|state| {
                    let mut state = state.borrow_mut();
                    match result {
                        Ok((session, stats)) => {
                            state.clipboard = Some(session);
                            state.stats = Some(stats);
                            state.error = None;
                        }
                        Err(error) => state.error = Some(error.to_string()),
                    }
                });
                refresh(hwnd);
            }
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            STATE.with(|state| state.borrow_mut().right_clicks += 1);
            // Uncomment to test cancellation during a paste operation.
            // Cancellation also affects subsequent pastes from the same clipboard session.
            //STATE.with(|state| {
            //    state.borrow_mut().clipboard.as_mut().unwrap().cancel();
            //});
            refresh(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_ID => {
            STATE.with(|state| state.borrow_mut().ticks += 1);
            refresh(hwnd);
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                _ = KillTimer(Some(hwnd), TIMER_ID);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn main() -> Result<()> {
    let module = unsafe { GetModuleHandleW(None)? };
    let instance = HINSTANCE(module.0);
    let class_name = w!("VirtualFileSlowClipboard");

    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: class_name,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&wc) } == 0 {
        return Err(Error::from_thread());
    }

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Slow Clipboard - transfer behavior test"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            760,
            390,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    if unsafe { SetTimer(Some(hwnd), TIMER_ID, 100, None) } == 0 {
        return Err(Error::from_thread());
    }

    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == -1 {
            return Err(Error::from_thread());
        }
        if result.0 == 0 {
            break;
        }
        unsafe {
            _ = TranslateMessage(&message);
            _ = DispatchMessageW(&message);
        }
    }

    STATE.with(|state| {
        let session = state.borrow_mut().clipboard.take();
        drop(session);
    });
    Ok(())
}
