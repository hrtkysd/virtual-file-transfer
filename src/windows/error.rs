use crate::{Error, PlatformError, PlatformErrorKind};
use windows::core::Error as WindowsError;
use windows::core::HRESULT;

pub(crate) fn from_windows_error(kind: PlatformErrorKind, error: windows::core::Error) -> Error {
    Error::Platform(PlatformError::new(kind, error.to_string()))
}

pub(crate) fn from_hresult(kind: PlatformErrorKind, hr: HRESULT) -> Error {
    from_windows_error(kind, windows::core::Error::from_hresult(hr))
}
