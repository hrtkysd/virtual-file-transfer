use super::{DataObject, DataObject_Impl};
use crate::transfer::{TransferEffect, TransferStatus};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::UI::Shell::IDataObjectAsyncCapability_Impl;
use windows::core::{BOOL, ComObject, Error as WindowsError, HRESULT, Ref, Result};
use windows_core::IUnknownImpl;

#[derive(Default)]
pub(super) struct AsyncState {
    pub enabled: bool,
    pub in_operation: bool,
    pub self_ref: Option<ComObject<DataObject>>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TransferResult {
    pub hr: HRESULT,
    pub effect: u32,
}

impl TransferResult {
    fn into_status(self) -> TransferStatus {
        use windows::Win32::System::Ole::{
            DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE,
        };

        let effect = match self.effect {
            x if x == DROPEFFECT_NONE.0 => TransferEffect::None,
            x if x == DROPEFFECT_COPY.0 => TransferEffect::Copy,
            x if x == DROPEFFECT_MOVE.0 => TransferEffect::Move,
            x if x == DROPEFFECT_LINK.0 => TransferEffect::Link,
            x => TransferEffect::Other(x),
        };

        TransferStatus::AsyncFinished {
            succeeded: self.hr.is_ok(),
            native_code: self.hr.0,
            effect,
        }
    }
}

#[allow(non_snake_case)]
impl IDataObjectAsyncCapability_Impl for DataObject_Impl {
    fn SetAsyncMode(&self, enabled: BOOL) -> Result<()> {
        let enabled = enabled.as_bool();
        let released = {
            let mut state = self
                .async_state
                .lock()
                .map_err(|_| WindowsError::from(E_FAIL))?;

            if state.in_operation && !enabled {
                return Err(E_FAIL.into());
            }

            state.enabled = enabled;

            if enabled {
                if state.self_ref.is_none() {
                    state.self_ref = Some(self.to_object());
                }
                None
            } else {
                state.self_ref.take()
            }
        };

        drop(released);
        Ok(())
    }

    fn GetAsyncMode(&self) -> Result<BOOL> {
        let state = self
            .async_state
            .lock()
            .map_err(|_| WindowsError::from(E_FAIL))?;

        Ok(BOOL::from(state.enabled))
    }

    fn StartOperation(&self, _reserved: Ref<'_, IBindCtx>) -> Result<()> {
        let mut state = self
            .async_state
            .lock()
            .map_err(|_| WindowsError::from(E_FAIL))?;

        if !state.enabled || state.in_operation {
            return Err(E_FAIL.into());
        }

        if state.self_ref.is_none() {
            state.self_ref = Some(self.to_object());
        }

        state.in_operation = true;
        Ok(())
    }

    fn InOperation(&self) -> Result<BOOL> {
        let state = self
            .async_state
            .lock()
            .map_err(|_| WindowsError::from(E_FAIL))?;

        Ok(BOOL::from(state.in_operation))
    }

    fn EndOperation(
        &self,
        result: HRESULT,
        _reserved: Ref<'_, IBindCtx>,
        effects: u32,
    ) -> Result<()> {
        let (released, record_result) = {
            // Recover the poisoned mutex to release the COM self-reference even if a previous operation panicked.
            let mut state = self
                .async_state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            if !state.in_operation {
                return Ok(());
            }

            let transfer_result = TransferResult {
                hr: result,
                effect: effects,
            };

            let record_result = if let Some(completion) = &self.completion {
                completion.record_if_pending(transfer_result.into_status())
            } else {
                Ok(())
            };

            state.in_operation = false;

            (state.self_ref.take(), record_result)
        };

        drop(released);

        record_result.map_err(|_| WindowsError::from(E_FAIL))
    }
}

#[cfg(test)]
mod tests {
    use super::super::DataObject;
    use crate::cancellation::CancellationToken;
    use crate::{FileSource, SourceReader, VirtualFile};
    use std::io;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::UI::Shell::IDataObjectAsyncCapability;
    use windows::core::{ComObject, Interface};

    struct StubSource;

    impl FileSource for StubSource {
        fn open(&self) -> io::Result<SourceReader> {
            Ok(SourceReader::seekable(io::Cursor::new(b"hello".to_vec())))
        }
    }

    fn create_async_object() -> IDataObjectAsyncCapability {
        let file = VirtualFile::new("hello.txt", Box::new(StubSource)).unwrap();

        let data_object: IDataObject = DataObject::new(vec![file], CancellationToken::new(), None)
            .unwrap()
            .into();

        data_object.cast().unwrap()
    }

    #[test]
    fn async_mode_is_disabled_by_default() {
        let object = create_async_object();

        assert!(!unsafe { object.GetAsyncMode().unwrap() }.as_bool());
    }

    #[test]
    fn async_mode_can_be_enabled_and_disabled() {
        let object = create_async_object();

        unsafe {
            object.SetAsyncMode(true).unwrap();
            assert!(object.GetAsyncMode().unwrap().as_bool());

            object.SetAsyncMode(false).unwrap();
            assert!(!object.GetAsyncMode().unwrap().as_bool());
        }
    }

    #[test]
    fn start_operation_requires_async_mode() {
        let object = create_async_object();

        assert!(unsafe { object.StartOperation(None) }.is_err());
    }

    #[test]
    fn operation_state_changes() {
        let object = create_async_object();

        unsafe {
            assert!(!object.InOperation().unwrap().as_bool());

            object.SetAsyncMode(true).unwrap();

            object.StartOperation(None).unwrap();
            assert!(object.InOperation().unwrap().as_bool());

            object.EndOperation(S_OK, None, 0).unwrap();
            assert!(!object.InOperation().unwrap().as_bool());
        }
    }

    fn create_async_object_with_owner() -> (ComObject<DataObject>, IDataObjectAsyncCapability) {
        let file = VirtualFile::new("hello.txt", Box::new(StubSource)).unwrap();

        let object =
            ComObject::new(DataObject::new(vec![file], CancellationToken::new(), None).unwrap());

        let capability = object.to_interface();

        (object, capability)
    }

    #[test]
    fn self_reference_is_reacquired_after_end_operation() {
        let (object, capability) = create_async_object_with_owner();

        unsafe {
            capability.SetAsyncMode(true).unwrap();
            capability.StartOperation(None).unwrap();
        }

        assert!(object.get().async_state.lock().unwrap().self_ref.is_some());

        unsafe {
            capability.EndOperation(S_OK, None, 0).unwrap();
        }

        {
            let state = object.get().async_state.lock().unwrap();
            assert!(!state.in_operation);
            assert!(state.self_ref.is_none());
        }

        // Start another asynchronous operation using the same DataObject.
        unsafe {
            capability.StartOperation(None).unwrap();
        }

        {
            let state = object.get().async_state.lock().unwrap();
            assert!(state.in_operation);
            assert!(state.self_ref.is_some());
        }

        unsafe {
            capability.EndOperation(S_OK, None, 0).unwrap();
        }

        assert!(object.get().async_state.lock().unwrap().self_ref.is_none());
    }

    #[test]
    fn active_operation_rejects_restart_and_disable() {
        let (object, capability) = create_async_object_with_owner();

        unsafe {
            capability.SetAsyncMode(true).unwrap();
            capability.StartOperation(None).unwrap();

            assert!(capability.StartOperation(None).is_err());
            assert!(capability.SetAsyncMode(false).is_err());

            assert!(capability.GetAsyncMode().unwrap().as_bool());
            assert!(capability.InOperation().unwrap().as_bool());
        }

        assert!(object.get().async_state.lock().unwrap().self_ref.is_some());

        unsafe {
            capability.EndOperation(S_OK, None, 0).unwrap();
        }

        assert!(object.get().async_state.lock().unwrap().self_ref.is_none());
    }

    #[test]
    fn repeated_end_operation_does_not_release_twice() {
        let (object, capability) = create_async_object_with_owner();

        unsafe {
            capability.SetAsyncMode(true).unwrap();
            capability.StartOperation(None).unwrap();

            capability.EndOperation(S_OK, None, 0).unwrap();
            capability.EndOperation(S_OK, None, 0).unwrap();
        }

        let state = object.get().async_state.lock().unwrap();

        assert!(!state.in_operation);
        assert!(state.self_ref.is_none());
    }

    #[test]
    fn finish_without_async_releases_self_reference() {
        let (object, capability) = create_async_object_with_owner();

        unsafe {
            capability.SetAsyncMode(true).unwrap();
        }

        assert!(object.get().async_state.lock().unwrap().self_ref.is_some());

        object.get().finish_without_async().unwrap();

        let state = object.get().async_state.lock().unwrap();

        assert!(!state.enabled);
        assert!(!state.in_operation);
        assert!(state.self_ref.is_none());
    }

    fn poison_async_state(object: &ComObject<DataObject>) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _guard = object.get().async_state.lock().unwrap();
            panic!("Intentionally poison async_state");
        }));

        assert!(result.is_err());
        assert!(object.get().async_state.is_poisoned());
    }

    #[test]
    fn finish_without_async_releases_self_reference_when_mutex_is_poisoned() {
        let (object, capability) = create_async_object_with_owner();

        unsafe {
            capability.SetAsyncMode(true).unwrap();
        }

        poison_async_state(&object);

        // Cleanup must succeed even if the mutex is poisoned.
        object.get().finish_without_async().unwrap();

        let state = object
            .get()
            .async_state
            .lock()
            .err()
            .expect("async_state should be poisoned")
            .into_inner();

        assert!(!state.enabled);
        assert!(!state.in_operation);
        assert!(state.self_ref.is_none());
    }

    #[test]
    fn end_operation_releases_self_reference_when_mutex_is_poisoned() {
        let (object, capability) = create_async_object_with_owner();

        unsafe {
            capability.SetAsyncMode(true).unwrap();
            capability.StartOperation(None).unwrap();
        }

        poison_async_state(&object);

        // EndOperation must release the self-reference even if
        // the mutex is poisoned.
        unsafe {
            capability.EndOperation(S_OK, None, 0).unwrap();
        }

        let state = object
            .get()
            .async_state
            .lock()
            .err()
            .expect("async_state should be poisoned")
            .into_inner();

        assert!(!state.in_operation);
        assert!(state.self_ref.is_none());
    }
}
