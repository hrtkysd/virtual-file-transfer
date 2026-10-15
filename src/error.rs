use std::fmt;
use std::io;

pub type Result<T> = std::result::Result<T, Error>;

#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    InvalidInput(InputError),
    Provider(io::Error),
    Platform(PlatformError),
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileNameErrorReason {
    InvalidCharacter(char),
    InvalidComponent(String),
    ReservedName(String),
    TrailingSpaceOrPeriod(String),
    TooLong { actual: usize, max: usize },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    ConflictPath,
    DuplicatePath,
    EmptyName,
    EmptyTransfer,
    InvalidFileName {
        name: String,
        reason: FileNameErrorReason,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformErrorKind {
    UnsupportedFileName,
    InitializationFailed,
    TransferFailed,
}

#[derive(Debug)]
pub struct PlatformError {
    kind: PlatformErrorKind,
    message: String,
}

impl PlatformError {
    pub(crate) fn new(kind: PlatformErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> PlatformErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Provider(error)
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "file name must not be empty"),
            Self::InvalidFileName { name, reason } => {
                write!(f, "invalid file name {name:?}: {reason}")
            }
            Self::EmptyTransfer => write!(f, "transfer must contain at least one file"),
            Self::DuplicatePath => write!(f, "duplicate file path"),
            Self::ConflictPath => write!(f, "file path conflicts with a parent path"),
        }
    }
}

impl fmt::Display for FileNameErrorReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCharacter(ch) => write!(f, "invalid character {ch:?}"),
            Self::InvalidComponent(component) => write!(f, "invalid path component {component:?}"),
            Self::ReservedName(component) => write!(f, "reserved device name {component:?}"),
            Self::TrailingSpaceOrPeriod(component) => {
                write!(f, "trailing space or period in {component:?}")
            }
            Self::TooLong { actual, max } => {
                write!(
                    f,
                    "path length {actual} exceeds maximum {max} UTF-16 code units"
                )
            }
        }
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(error) => {
                write!(f, "invalid input: {error}")
            }

            Self::Provider(error) => {
                write!(f, "provider error: {error}")
            }

            Self::Platform(error) => {
                write!(f, "platform error: {error}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Provider(error) => Some(error),
            _ => None,
        }
    }
}
