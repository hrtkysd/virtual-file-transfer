use ouroboros::self_referencing;
use promised_file::{FileSource, SourceReader, begin_drag};

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetMessageW, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassW, TranslateMessage,
    WINDOW_EX_STYLE, WM_DESTROY, WM_LBUTTONDOWN, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::{Error as WindowsError, w};
use zip::read::{ZipArchive, ZipFile};

#[self_referencing]
struct ZipEntryReaderCell {
    archive: ZipArchive<File>,

    #[borrows(mut archive)]
    #[not_covariant]
    entry: ZipFile<'this, File>,
}

struct LazyZipEntryReader {
    cell: ZipEntryReaderCell,
}

impl LazyZipEntryReader {
    fn open(archive_path: &Path, entry_name: &str) -> io::Result<Self> {
        let file = File::open(archive_path)?;

        let archive = ZipArchive::new(file).map_err(zip_error)?;

        let entry_name = entry_name.to_owned();

        let cell = ZipEntryReaderCell::try_new(archive, move |archive| {
            archive.by_name(&entry_name).map_err(zip_error)
        })?;

        Ok(Self { cell })
    }
}

impl Read for LazyZipEntryReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.cell.with_entry_mut(|entry| entry.read(buffer))
    }
}

struct ZipEntrySource {
    archive_path: PathBuf,
    entry_name: String,
}

impl ZipEntrySource {
    fn new(archive_path: impl Into<PathBuf>, entry_name: impl Into<String>) -> Self {
        Self {
            archive_path: archive_path.into(),
            entry_name: entry_name.into(),
        }
    }
}

impl FileSource for ZipEntrySource {
    fn open(&self) -> io::Result<SourceReader> {
        let reader = LazyZipEntryReader::open(&self.archive_path, &self.entry_name)?;

        Ok(SourceReader::streaming(reader))
    }
}

fn zip_error(error: zip::result::ZipError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn create_zip_virtual_files(
    archive_path: PathBuf,
    entry_names: &[&str],
) -> std::result::Result<Vec<promised_file::VirtualFile>, Box<dyn std::error::Error>> {
    let file = File::open(&archive_path)?;
    let mut archive = ZipArchive::new(file)?;

    let mut files = Vec::with_capacity(entry_names.len());

    for entry_name in entry_names {
        let entry = archive.by_name(entry_name)?;

        let size = entry.size();

        let file_name = Path::new(entry.name())
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid ZIP entry name"))?
            .to_owned();

        drop(entry);

        let source = ZipEntrySource::new(archive_path.clone(), *entry_name);

        let file = promised_file::VirtualFile::new(&file_name, Box::new(source))?.with_size(size);

        files.push(file);
    }

    Ok(files)
}
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_LBUTTONDOWN => {
            match create_zip_virtual_files(PathBuf::from("sample.zip"), &["hello.txt", "world.txt"])
            {
                Ok(files) => {
                    if let Err(error) = begin_drag(files) {
                        eprintln!("drag failed: {error}");
                    }
                }

                Err(error) => {
                    eprintln!("failed to create virtual file: {error}");
                }
            }

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

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
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
        return Err(Box::new(WindowsError::from_thread()));
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
            return Err(Box::new(WindowsError::from_thread()));
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
