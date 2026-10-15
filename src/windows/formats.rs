use std::mem::{ManuallyDrop, size_of};

use std::ptr;

use std::sync::OnceLock;

use windows::Win32::Foundation::{E_INVALIDARG, GlobalFree};
use windows::Win32::System::Com::{STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::UI::Shell::{FD_FILESIZE, FD_UNICODE, FILEDESCRIPTORW};
use windows::core::{Error as WindowsError, Result as WindowsResult, w};

pub(crate) struct FileDescriptorMetadata<'a> {
    pub(crate) name: &'a str,
    pub(crate) size: Option<u64>,
}

pub(crate) struct FileTransferFormats {
    pub(crate) file_descriptor: u32,
    pub(crate) file_contents: u32,
}

static FORMATS: OnceLock<FileTransferFormats> = OnceLock::new();

pub(crate) fn file_transfer_formats() -> WindowsResult<&'static FileTransferFormats> {
    if let Some(formats) = FORMATS.get() {
        return Ok(formats);
    }

    let formats = FileTransferFormats::register()?;

    Ok(FORMATS.get_or_init(|| formats))
}

impl FileTransferFormats {
    fn register() -> WindowsResult<Self> {
        let file_descriptor = unsafe { RegisterClipboardFormatW(w!("FileGroupDescriptorW")) };

        if file_descriptor == 0 {
            return Err(WindowsError::from_thread());
        }

        let file_contents = unsafe { RegisterClipboardFormatW(w!("FileContents")) };

        if file_contents == 0 {
            return Err(WindowsError::from_thread());
        }

        Ok(Self {
            file_descriptor,
            file_contents,
        })
    }
}

fn create_file_descriptor(metadata: &FileDescriptorMetadata<'_>) -> WindowsResult<FILEDESCRIPTORW> {
    let mut descriptor = FILEDESCRIPTORW::default();

    let mut flags = FD_UNICODE.0 as u32;

    if let Some(size) = metadata.size {
        flags |= FD_FILESIZE.0 as u32;

        descriptor.nFileSizeHigh = (size >> 32) as u32;

        descriptor.nFileSizeLow = size as u32;
    }

    descriptor.dwFlags = flags;

    let normalized_name = metadata.name.replace('/', "\\");
    let name_utf16: Vec<u16> = normalized_name.encode_utf16().collect();

    let mut file_name = [0u16; 260];

    if name_utf16.len() >= file_name.len() {
        return Err(WindowsError::new(
            E_INVALIDARG,
            "file name exceeds FILEDESCRIPTORW capacity",
        ));
    }

    file_name[..name_utf16.len()].copy_from_slice(&name_utf16);
    descriptor.cFileName = file_name;

    Ok(descriptor)
}

pub(crate) fn create_file_group_descriptor_medium(
    files: &[FileDescriptorMetadata<'_>],
) -> WindowsResult<STGMEDIUM> {
    let descriptors = files
        .iter()
        .map(create_file_descriptor)
        .collect::<WindowsResult<Vec<_>>>()?;
    let descriptors_size = descriptors.len() * size_of::<FILEDESCRIPTORW>();

    let byte_size = size_of::<u32>() + descriptors_size;

    let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_size) }?;

    let destination = unsafe { GlobalLock(hglobal) };

    if destination.is_null() {
        unsafe {
            _ = GlobalFree(Some(hglobal));
        }

        return Err(WindowsError::from_thread());
    }

    unsafe {
        ptr::write_unaligned(destination.cast::<u32>(), descriptors.len() as u32);

        ptr::copy_nonoverlapping(
            descriptors.as_ptr().cast::<u8>(),
            destination.cast::<u8>().add(size_of::<u32>()),
            descriptors_size,
        );

        _ = GlobalUnlock(hglobal);
    }

    Ok(STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,

        u: STGMEDIUM_0 { hGlobal: hglobal },

        pUnkForRelease: ManuallyDrop::new(None),
    })
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn creates_file_descriptor() {
        let metadata = FileDescriptorMetadata {
            name: "hello.txt",
            size: Some(5),
        };

        let descriptor = create_file_descriptor(&metadata).unwrap();

        let size_high = descriptor.nFileSizeHigh;
        let size_low = descriptor.nFileSizeLow;

        let size = ((size_high as u64) << 32) | size_low as u64;

        assert_eq!(size, 5);

        let file_name = descriptor.cFileName;
        let name_len = file_name.iter().position(|&c| c == 0).unwrap();
        let name = String::from_utf16(&file_name[..name_len]).unwrap();

        assert_eq!(name, "hello.txt");
    }

    #[test]
    fn normalizes_relative_path_in_descriptor() {
        let metadata = FileDescriptorMetadata {
            name: "reports/2026/report.csv",
            size: Some(5),
        };

        let descriptor = create_file_descriptor(&metadata).unwrap();

        let file_name = descriptor.cFileName;
        let length = file_name.iter().position(|&c| c == 0).unwrap();
        let name = String::from_utf16(&file_name[..length]).unwrap();

        assert_eq!(name, r"reports\2026\report.csv");
    }

    #[test]
    fn validates_descriptor_name_length() {
        // 259 UTF-16 code units + NUL: valid
        let valid_name = "a".repeat(259);
        let metadata = FileDescriptorMetadata {
            name: &valid_name,
            size: None,
        };
        assert!(create_file_descriptor(&metadata).is_ok());

        // 260 UTF-16 code units + NUL: invalid
        let invalid_name = "a".repeat(260);
        let metadata = FileDescriptorMetadata {
            name: &invalid_name,
            size: None,
        };
        assert!(create_file_descriptor(&metadata).is_err());
    }
}
