//! Virtual file transfer through Windows drag-and-drop and
//! the clipboard.
//!
//! This crate allows applications to offer data as files
//! without first writing the contents to temporary files.
//!
//! File contents are provided on demand using [`FileSource`]
//! and [`SourceReader`]. On Windows, these are exposed to
//! receiving applications through OLE `IDataObject` and
//! `IStream` interfaces.
//!
//! # Quick start
//!
//! ```no_run
//! use std::io::Cursor;
//! use virtual_file_transfer::{
//!     begin_drag, FactorySource, SourceReader, VirtualFile,
//! };
//!
//! fn start_drag() -> virtual_file_transfer::Result<()> {
//!     let source = FactorySource::new(|| {
//!         Ok(SourceReader::seekable(
//!             Cursor::new(b"hello\n".to_vec())
//!         ))
//!     });
//!
//!     let file = VirtualFile::new(
//!         "hello.txt",
//!         Box::new(source),
//!     )?;
//!
//!     let _handle = begin_drag(vec![file])?;
//!     Ok(())
//! }
//! ```
//!
//! Call [`begin_drag`] from an appropriate Windows UI event
//! handler. The application must provide a message loop.
//!
//! # Data sources
//!
//! A [`FileSource`] produces a fresh [`SourceReader`] for
//! each request. Sources may read from memory, archives,
//! generated content, or other application-defined storage.
//!
//! Use [`SourceReader::streaming`] for sequential data and
//! [`SourceReader::seekable`] when seeking is available.
//!
//! The optional `spool` feature provides
//! [`SourceReader::spooled`] for explicitly materializing
//! non-seekable data into temporary storage.
//!
//! # Transfer operations
//!
//! - [`begin_drag`] starts an OLE drag-and-drop operation.
//! - [`copy_to_clipboard`] exposes virtual files through
//!   the system clipboard.
//! - [`TransferHandle`] provides cancellation and status
//!   information.
//!
//! A successful drop does not necessarily mean that an
//! asynchronous receiver has finished reading the data.
//! See [`TransferStatus`] for completion semantics.
//!
//! # Platform support
//!
//! Currently Windows only.
mod cancellation;
mod error;
mod factory_source;
mod file_source;
mod source_reader;
mod transfer;
mod validation;
mod virtual_file;

/// Windows-specific APIs, including the lower-level OLE data-object interface.
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

/// Starts a Windows OLE drag-and-drop operation for the supplied virtual files.
///
/// This calls `DoDragDrop` on the current thread and returns when the drag
/// operation ends. The receiver may continue fetching file contents
/// asynchronously after this function returns.
///
/// Call this from a suitable Windows UI thread with a message loop. The
/// function initializes OLE for the duration of the drag operation.
///
/// The request must contain at least one file and must not contain duplicate
/// or conflicting paths. A `Vec<VirtualFile>` can be passed directly.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] for invalid transfers or
/// [`Error::Platform`] if OLE initialization or drag-and-drop fails.
#[cfg(windows)]
pub fn begin_drag(request: impl Into<TransferRequest>) -> Result<TransferHandle> {
    windows::begin_drag(request.into())
}

/// Keeps a virtual-file clipboard transfer available.
///
/// The session owns the resources needed to serve file
/// contents when another application pastes them.
///
/// Keep this value alive while clipboard data may still
/// be requested.
///
/// Dropping the session clears its clipboard contents
/// if they are still the current clipboard contents.
/// It does not clear data installed by another application
/// or a newer clipboard session.
#[cfg(windows)]
#[must_use = "Keep this session alive while clipboard data is needed"]
pub struct ClipboardSession {
    inner: windows::ClipboardSession,
}

#[cfg(windows)]
impl ClipboardSession {
    /// Requests cancellation of reads served by this clipboard session.
    ///
    /// Cancellation is cooperative: an already-running blocking read is not
    /// forcibly interrupted. Subsequent paste attempts using the same session
    /// can also observe cancellation.
    pub fn cancel(&self) {
        self.inner.cancel();
    }
}

/// Publishes virtual files through the Windows clipboard.
///
/// The returned [`ClipboardSession`] must remain alive while the data is
/// available for pasting. File contents are provided lazily when the receiver
/// requests them. A `Vec<VirtualFile>` can be passed directly.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] for an invalid transfer or
/// [`Error::Platform`] if OLE initialization or clipboard registration fails.
#[cfg(windows)]
pub fn copy_to_clipboard(request: impl Into<TransferRequest>) -> Result<ClipboardSession> {
    Ok(ClipboardSession {
        inner: windows::copy_to_clipboard(request.into())?,
    })
}
