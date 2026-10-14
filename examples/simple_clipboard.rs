use std::io;

use virtual_file_transfer::{
    FileSource, SourceReader, TransferRequest, VirtualFile, copy_to_clipboard,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, TranslateMessage,
};

struct MemorySource {
    data: Vec<u8>,
}

impl FileSource for MemorySource {
    fn open(&self) -> io::Result<SourceReader> {
        Ok(SourceReader::seekable(std::io::Cursor::new(
            self.data.clone(),
        )))
    }
}

fn main() -> virtual_file_transfer::Result<()> {
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
    let _clipboard = copy_to_clipboard(TransferRequest::from(files))?;

    println!("Paste into Explorer with Ctrl+V.");
    println!("Press Enter to exit.");

    let mut message = MSG::default();

    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };

        if result.0 == -1 {
            break;
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
