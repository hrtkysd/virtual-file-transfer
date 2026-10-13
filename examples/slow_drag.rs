use std::{
    cell::RefCell,
    io::{self, Read},
    thread,
    time::Duration,
};

use virtual_file_transfer::{
    FactorySource, SourceReader, TransferHandle, TransferStatus, VirtualFile, begin_drag,
};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT, TextOutW};
use windows::Win32::System::{
    LibraryLoader::GetModuleHandleW,
    Ole::{OleInitialize, OleUninitialize},
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetMessageW, IDC_ARROW, KillTimer, LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SetTimer,
    TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WM_LBUTTONDOWN, WM_PAINT,
    WM_RBUTTONDOWN, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::{Error, Result, w};

const FILE_SIZE: u64 = 2 * 1024 * 1024; // 2MiB
const CHUNK_SIZE: usize = 64 * 1024; // 64KiB
const READ_DELAY: Duration = Duration::from_millis(120); // ChunkDuration
const TIMER_ID: usize = 1;

struct SlowReader {
    remaining: u64,
}

impl Read for SlowReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = buffer.len().min(CHUNK_SIZE).min(self.remaining as usize);

        if count == 0 {
            return Ok(0);
        }

        thread::sleep(READ_DELAY);
        buffer[..count].fill(b'X');
        self.remaining -= count as u64;

        eprintln!(
            "[SlowReader] thread={:?}, bytes={count}, remaining={}",
            thread::current().id(),
            self.remaining,
        );
        Ok(count)
    }
}

fn start_drag() -> virtual_file_transfer::Result<TransferHandle> {
    let source = FactorySource::new(|| {
        Ok(SourceReader::streaming(SlowReader {
            remaining: FILE_SIZE,
        }))
    });
    let file = VirtualFile::new("slow-test.bin", Box::new(source))?.with_size(FILE_SIZE);
    begin_drag(vec![file])
}

#[derive(Default)]
struct AppState {
    ticks: u64,
    right_clicks: u64,
    transfer: Option<TransferHandle>,
    error: Option<String>,
}

thread_local! {
    static STATE: RefCell<AppState> = RefCell::new(AppState::default());
}

fn transfer_is_pending() -> bool {
    STATE.with(|state| {
        state
            .borrow()
            .transfer
            .as_ref()
            .is_some_and(|handle| matches!(handle.status(), TransferStatus::Pending))
    })
}

fn refresh(hwnd: HWND) {
    unsafe {
        _ = InvalidateRect(Some(hwnd), None, true);
    }
}

fn paint(hwnd: HWND) {
    let lines = STATE.with(|state| {
        let state = state.borrow();
        let status = state.transfer.as_ref().map_or_else(
            || "Not started".to_owned(),
            |handle| format!("{:?}", handle.status()),
        );

        [
            "Left-click and drag this window to Explorer.".to_owned(),
            "Right-click while transferring to test UI responsiveness.".to_owned(),
            format!("Heartbeat (100 ms ticks): {}", state.ticks),
            format!("Right clicks: {}", state.right_clicks),
            format!("Transfer status: {status}"),
            format!("Last error: {}", state.error.as_deref().unwrap_or("None")),
        ]
    });

    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };

    for (index, line) in lines.iter().enumerate() {
        let chars: Vec<u16> = line.encode_utf16().collect();
        unsafe {
            _ = TextOutW(hdc, 16, 24 + index as i32 * 32, &chars);
        }
    }

    unsafe {
        _ = EndPaint(hwnd, &paint);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_LBUTTONDOWN => {
            if !transfer_is_pending() {
                let result = start_drag();
                STATE.with(|state| {
                    let mut state = state.borrow_mut();
                    match result {
                        Ok(handle) => {
                            state.transfer = Some(handle);
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
        WM_CLOSE if transfer_is_pending() => {
            eprintln!("Transfer still pending; close after EndOperation");
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

struct OleGuard;

impl OleGuard {
    fn new() -> Result<Self> {
        unsafe { OleInitialize(None)? };
        Ok(Self)
    }
}

impl Drop for OleGuard {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
    }
}

fn main() -> Result<()> {
    let _ole = OleGuard::new()?;

    let module = unsafe { GetModuleHandleW(None)? };
    let instance = HINSTANCE(module.0);
    let class_name = w!("VirtualFileAsynchronousDrag");

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
            w!("Low Performance Drag - asynchronous transfer test"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            730,
            320,
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
    Ok(())
}
