mod error;
mod factory_source;
mod file_source;
mod source_reader;
mod virtual_file;

#[cfg(windows)]
pub mod windows;

pub use error::{Error, InputError, PlatformError, PlatformErrorKind, Result};
pub use factory_source::FactorySource;
pub use file_source::FileSource;
pub use source_reader::SourceReader;
pub use virtual_file::VirtualFile;

#[cfg(windows)]
pub fn begin_drag(files: Vec<VirtualFile>) -> Result<()> {
    windows::begin_drag(files)
}

#[cfg(windows)]
pub struct ClipboardSession {
    _inner: windows::ClipboardSession,
}

#[cfg(windows)]
pub fn copy_to_clipboard(file: Vec<VirtualFile>) -> Result<ClipboardSession> {
    Ok(ClipboardSession {
        _inner: windows::copy_to_clipboard(file)?,
    })
}
