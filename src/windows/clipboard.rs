use super::OleContext;
use super::data_object::DataObject;
use super::error::from_windows_error;

use crate::{PlatformErrorKind, Result, VirtualFile};

use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::OleSetClipboard;

pub(crate) struct ClipboardSession {
    _ole: OleContext,
}

pub(crate) fn copy_to_clipboard(files: Vec<VirtualFile>) -> Result<ClipboardSession> {
    let ole = OleContext::new()
        .map_err(|error| from_windows_error(PlatformErrorKind::InitializationFailed, error))?;

    let data_object: IDataObject = DataObject::new(files)
        .map_err(|error| from_windows_error(PlatformErrorKind::TransferFailed, error))?
        .into();

    unsafe { OleSetClipboard(&data_object) }
        .map_err(|error| from_windows_error(PlatformErrorKind::TransferFailed, error))?;

    Ok(ClipboardSession { _ole: ole })
}
