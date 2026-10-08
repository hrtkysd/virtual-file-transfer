use super::OleContext;
use super::data_object::DataObject;
use super::drop_source::DropSource;
use super::error::{from_hresult, from_windows_error};

use crate::{PlatformErrorKind, Result, VirtualFile};

use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE, DoDragDrop, IDropSource};

pub(crate) fn begin_drag(files: Vec<VirtualFile>) -> Result<()> {
    let _ole = OleContext::new()
        .map_err(|error| from_windows_error(PlatformErrorKind::InitializationFailed, error))?;

    let data_object: IDataObject = DataObject::new(files)
        .map_err(|error| from_windows_error(PlatformErrorKind::InitializationFailed, error))?
        .into();

    let drop_source: IDropSource = DropSource.into();

    let mut effect = DROPEFFECT_NONE;

    let result = unsafe { DoDragDrop(&data_object, &drop_source, DROPEFFECT_COPY, &mut effect) };

    if result.is_err() {
        return Err(from_hresult(PlatformErrorKind::TransferFailed, result));
    }

    Ok(())
}
