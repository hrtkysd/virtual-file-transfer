mod cancellation;
mod error;
mod factory_source;
mod file_source;
mod source_reader;
mod transfer;
mod validation;
mod virtual_file;

#[cfg(windows)]
pub mod windows;

pub use cancellation::CancellationToken;
pub use error::{Error, FileNameErrorReason, InputError, PlatformError, PlatformErrorKind, Result};
pub use factory_source::FactorySource;
pub use file_source::FileSource;
pub use source_reader::SourceReader;
pub use transfer::{
    NativeStatusCode, TransferEffect, TransferHandle, TransferRequest, TransferStatus,
};
pub use virtual_file::VirtualFile;

#[cfg(windows)]
pub fn begin_drag(request: impl Into<TransferRequest>) -> Result<TransferHandle> {
    windows::begin_drag(request.into())
}

#[cfg(windows)]
#[must_use = "Keep this session alive while clipboard data is needed"]
pub struct ClipboardSession {
    inner: windows::ClipboardSession,
}

#[cfg(windows)]
impl ClipboardSession {
    pub fn cancel(&self) {
        self.inner.cancel();
    }
}

#[cfg(windows)]
pub fn copy_to_clipboard(request: impl Into<TransferRequest>) -> Result<ClipboardSession> {
    Ok(ClipboardSession {
        inner: windows::copy_to_clipboard(request.into())?,
    })
}
