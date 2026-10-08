use promised_file::{FileSource, SourceReader, VirtualFile, begin_drag};

use std::io;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetMessageW, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassW, TranslateMessage,
    WINDOW_EX_STYLE, WM_DESTROY, WM_LBUTTONDOWN, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::{Error, Result, w};

struct MemorySource {
    data: Vec<u8>,
}

impl FileSource for MemorySource {
    fn open(&self) -> io::Result<SourceReader> {
        Ok(SourceReader::streaming(std::io::Cursor::new(
            self.data.clone(),
        )))
    }
}
fn begin_drag_example() -> promised_file::Result<()> {
    let files = vec![
        VirtualFile::new(
            "hello.txt",
            Box::new(MemorySource {
                data: b"hello".to_vec(),
            }),
        )?,
        VirtualFile::new(
            "world.txt",
            Box::new(MemorySource {
                data: b"world".to_vec(),
            }),
        )?,
    ];

    begin_drag(files)?;

    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_LBUTTONDOWN => {
            eprintln!("WM_LBUTTONDOWN");

            if let Err(error) = begin_drag_example() {
                eprintln!("begin_drag failed: {error}");
            }

            eprintln!("start_drag returned");

            LRESULT(0)
        }

        WM_DESTROY => {
            unsafe {
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

    let class_name = w!("PromisedFileSimpleDrag");

    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: class_name,
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&window_class) };

    if atom == 0 {
        return Err(Error::from_thread());
    }

    let _window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("PromisedFile - click and drag to Explorer"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            600,
            300,
            None,
            None,
            Some(instance),
            None,
        )?
    };

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
