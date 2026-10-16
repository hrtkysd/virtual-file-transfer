use std::fmt;
use std::io;

/// Result type used by this crate's public fallible operations.
pub type Result<T> = std::result::Result<T, Error>;

/// An error returned by a public transfer or file-definition operation.
///
/// Input errors describe invalid file names or transfer requests. Provider
/// errors originate in I/O operations, while platform errors describe
/// operating-system integration failures.
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// Invalid file name, path, or transfer request.
    InvalidInput(InputError),
    /// An I/O error originating from a content provider.
    Provider(io::Error),
    /// An error from platform initialization or transfer integration.
    Platform(PlatformError),
}

/// The reason a virtual file name or path component is invalid.
///
/// Windows-specific restrictions include reserved device names, prohibited
/// characters, and the UTF-16 capacity of `FILEDESCRIPTORW`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileNameErrorReason {
    /// A prohibited character was found in a path component.
    InvalidCharacter(char),
    /// A path component is invalid, such as an empty or traversal component.
    InvalidComponent(String),
    /// A path component uses a reserved Windows device name.
    ReservedName(String),
    /// A Windows path component ends with a space or period.
    TrailingSpaceOrPeriod(String),
    /// The file path exceeds the supported length in UTF-16 code units.
    ///
    /// `actual` is the encoded length and `max` is the supported maximum.
    TooLong { actual: usize, max: usize },
}

/// An invalid file definition or transfer request supplied by the caller.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    /// A file path conflicts with another path treated as a parent directory.
    ConflictPath,
    /// Two files in the same transfer have duplicate destination paths.
    DuplicatePath,
    /// A virtual file was constructed with an empty name.
    EmptyName,
    /// A transfer request contains no virtual files.
    EmptyTransfer,
    /// A file name or relative path violates validation rules.
    InvalidFileName {
        /// The rejected name or relative path.
        name: String,
        /// The validation rule that was violated.
        reason: FileNameErrorReason,
    },
}

/// The category of an operating-system integration error.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformErrorKind {
    /// The platform cannot represent or accept a file name.
    UnsupportedFileName,
    /// Initialization of required platform facilities failed.
    InitializationFailed,
    /// A platform transfer or clipboard operation failed.
    TransferFailed,
}

/// A platform-specific failure with a categorized kind and diagnostic message.
///
/// Use [`Self::kind`] to inspect the category and [`Self::message`] for
/// additional details supplied by the platform adapter.
#[derive(Debug)]
pub struct PlatformError {
    /// Returns the category of this platform error.
    kind: PlatformErrorKind,
    /// Returns the diagnostic message describing this platform error.
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
