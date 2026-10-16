use virtual_file_transfer::{FactorySource, SourceReader, VirtualFile, begin_drag};

use std::io::{self, Read};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetMessageW, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassW, TranslateMessage,
    WINDOW_EX_STYLE, WM_DESTROY, WM_LBUTTONDOWN, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::{Error, Result, w};

const TEST_SIZE: u64 = (1u64 << 32) + 1;
const BLOCK_SIZE: u64 = 1024 * 1024;

struct GeneratedReader {
    position: u64,
}

impl Read for GeneratedReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = TEST_SIZE - self.position;
        let length = (buffer.len() as u64).min(remaining) as usize;

        let mut written = 0;

        while written < length {
            let block_index = self.position / BLOCK_SIZE;
            let block_offset = self.position % BLOCK_SIZE;

            let chunk = (length - written).min((BLOCK_SIZE - block_offset) as usize);

            buffer[written..written + chunk].fill((block_index % 251) as u8);

            self.position += chunk as u64;
            written += chunk;
        }

        Ok(length)
    }
}

fn begin_drag_example() -> virtual_file_transfer::Result<()> {
    let source =
        FactorySource::new(|| Ok(SourceReader::streaming(GeneratedReader { position: 0 })));

    let file = VirtualFile::new("large-test.bin", Box::new(source))?.with_size(TEST_SIZE);

    begin_drag(vec![file])?;

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

    let class_name = w!("VirtualFileLargeDrag");

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
            w!("Large File Drag - click and drag to Explorer"),
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
