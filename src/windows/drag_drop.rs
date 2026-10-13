use super::OleContext;
use super::data_object::DataObject;
use super::drop_source::DropSource;
use super::error::{from_hresult, from_windows_error};

use crate::transfer::{TransferHandle, TransferStatus};
use crate::{PlatformErrorKind, Result, VirtualFile};

use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, E_FAIL};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE, DoDragDrop, IDropSource};
use windows::Win32::UI::Shell::IDataObjectAsyncCapability;
use windows::core::{ComObject, Error as WindowsError};

pub(crate) fn begin_drag(files: Vec<VirtualFile>) -> Result<TransferHandle> {
    let _ole = OleContext::new()
        .map_err(|error| from_windows_error(PlatformErrorKind::InitializationFailed, error))?;

    let object = ComObject::new(
        DataObject::new(files)
            .map_err(|error| from_windows_error(PlatformErrorKind::InitializationFailed, error))?,
    );

    let completion = object.get().completion();

    let data_object: IDataObject = object.to_interface();
    let async_capability: IDataObjectAsyncCapability = object.to_interface();

    let drop_source: IDropSource = DropSource.into();
    let mut effect = DROPEFFECT_NONE;

    unsafe { async_capability.SetAsyncMode(true) }
        .map_err(|error| from_windows_error(PlatformErrorKind::TransferFailed, error))?;

    let result = unsafe { DoDragDrop(&data_object, &drop_source, DROPEFFECT_COPY, &mut effect) };

    let in_operation = unsafe { async_capability.InOperation() }
        .map(|value| value.as_bool())
        .unwrap_or(false);
    if !in_operation {
        let cleanup_result = object.get().finish_without_async();

        let status = if result == DRAGDROP_S_CANCEL {
            TransferStatus::Cancelled
        } else if result.is_err() {
            TransferStatus::Failed
        } else {
            TransferStatus::Dropped
        };

        let record_result = completion.record_if_pending(status);

        cleanup_result
            .map_err(|error| from_windows_error(PlatformErrorKind::TransferFailed, error))?;

        record_result.map_err(|_| {
            from_windows_error(
                PlatformErrorKind::TransferFailed,
                WindowsError::from(E_FAIL),
            )
        })?;
    }

    if result.is_err() {
        return Err(from_hresult(PlatformErrorKind::TransferFailed, result));
    }

    Ok(TransferHandle::new(completion))
}
