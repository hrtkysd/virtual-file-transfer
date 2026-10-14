use super::OleContext;
use super::data_object::DataObject;
use super::error::from_windows_error;

use crate::cancellation::CancellationToken;
use crate::{PlatformErrorKind, Result, TransferRequest};

use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::OleSetClipboard;

pub(crate) struct ClipboardSession {
    _ole: OleContext,
    cancellation: CancellationToken,
}

impl ClipboardSession {
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}

pub(crate) fn copy_to_clipboard(request: TransferRequest) -> Result<ClipboardSession> {
    let ole = OleContext::new()
        .map_err(|error| from_windows_error(PlatformErrorKind::InitializationFailed, error))?;
    let TransferRequest {
        files,
        cancellation: token,
    } = request;

    let data_object: IDataObject = DataObject::new(files, token.clone(), None)
        .map_err(|error| from_windows_error(PlatformErrorKind::TransferFailed, error))?
        .into();

    // Unlike drag-and-drop, OleSetClipboard only registers the data object.
    // File contents are retrieved on demand when pasted, so asynchronous mode is not required for clipboard registration.
    unsafe { OleSetClipboard(&data_object) }
        .map_err(|error| from_windows_error(PlatformErrorKind::TransferFailed, error))?;

    Ok(ClipboardSession {
        _ole: ole,
        cancellation: token,
    })
}
