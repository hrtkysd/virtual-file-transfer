use crate::Result;
use std::mem::{ManuallyDrop, size_of};

use std::ptr;

use std::sync::OnceLock;

use windows::Win32::Foundation::{E_INVALIDARG, GlobalFree, MAX_PATH};
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
    validate_file_name(metadata.name)
        .map_err(|error| WindowsError::new(E_INVALIDARG, error.to_string()))?;

    let mut descriptor = FILEDESCRIPTORW::default();

    let mut flags = FD_UNICODE.0 as u32;

    if let Some(size) = metadata.size {
        flags |= FD_FILESIZE.0 as u32;

        descriptor.nFileSizeHigh = (size >> 32) as u32;

        descriptor.nFileSizeLow = size as u32;
    }

    descriptor.dwFlags = flags;

    let name_utf16: Vec<u16> = metadata.name.encode_utf16().collect();

    let mut file_name = [0u16; 260];

    assert!(name_utf16.len() < file_name.len());

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
            let _ = GlobalFree(Some(hglobal));
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

        let _ = GlobalUnlock(hglobal);
    }

    Ok(STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,

        u: STGMEDIUM_0 { hGlobal: hglobal },

        pUnkForRelease: ManuallyDrop::new(None),
    })
}

pub(crate) fn validate_file_name(name: &str) -> Result<()> {
    if name.encode_utf16().count() >= MAX_PATH as usize {
        return Err(crate::Error::Platform(crate::PlatformError::new(
            crate::PlatformErrorKind::UnsupportedFileName,
            format!("file name exceeds {} UTF-16 code units", MAX_PATH - 1),
        )));
    }

    Ok(())
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
    fn validates_file_name_length() {
        assert!(validate_file_name(&"a".repeat(259)).is_ok());
        assert!(validate_file_name(&"a".repeat(260)).is_err());

        assert!(validate_file_name(&"😀".repeat(130)).is_err());
    }
}
