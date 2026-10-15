mod clipboard;
mod data_object;
mod drag_drop;
mod drop_source;
mod error;
mod formats;
mod stream;

pub(crate) mod validation;

use std::marker::PhantomData;
use std::rc::Rc;

use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};

use crate::TransferRequest;

pub(crate) use self::clipboard::{ClipboardSession, copy_to_clipboard};
pub(crate) use self::drag_drop::begin_drag;

pub fn create_data_object(request: impl Into<TransferRequest>) -> crate::Result<IDataObject> {
    data_object::create_data_object(request.into())
}

pub(crate) struct OleContext {
    _thread_affinity: PhantomData<Rc<()>>,
}

impl OleContext {
    pub(crate) fn new() -> windows::core::Result<Self> {
        unsafe {
            OleInitialize(None)?;
        }

        Ok(Self {
            _thread_affinity: PhantomData,
        })
    }
}

impl Drop for OleContext {
    fn drop(&mut self) {
        unsafe {
            OleUninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_ole() {
        let _ole = OleContext::new().unwrap();
    }
}
