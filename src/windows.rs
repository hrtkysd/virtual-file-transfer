//! Windows OLE integration for virtual-file transfer.
//!
//! Most applications should use the crate-level [`crate::begin_drag`]
//! and [`crate::copy_to_clipboard`] convenience functions.
//! This module also exposes a lower-level
//! [`crate::windows::create_data_object`] function for applications
//! that manage their own drag-and-drop flow.
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

/// Creates an OLE `IDataObject` advertising the request's virtual files.
///
/// Use this when the application already manages its own `IDropSource`,
/// `DoDragDrop` invocation, and transfer effects. The caller is responsible
/// for appropriate COM/OLE initialization, threading, and object lifetimes.
/// A `Vec<VirtualFile>` can be passed directly.
///
/// Unlike [`begin_drag`](crate::begin_drag), this function only constructs
/// the data object; it does not start a drag operation.
///
/// # Errors
///
/// Returns [`crate::Error::InvalidInput`] for invalid transfer requests or
/// [`crate::Error::Platform`] for a platform integration failure.
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
