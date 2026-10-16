use crate::Result;
use crate::file_source::FileSource;
use crate::source_reader::SourceReader;
use crate::validation::validate_file_name;
use std::io;

/// Describes one virtual file exposed to a receiving application.
///
/// A virtual file consists of a name, an optional known size, and a
/// [`crate::FileSource`] that supplies its contents on demand.
/// The file need not exist as a physical file on the source machine.
pub struct VirtualFile {
    name: String,
    size: Option<u64>,
    source: Box<dyn FileSource>,
}

impl VirtualFile {
    /// Creates a virtual file with an unknown size.
    ///
    /// `name` may be a relative path such as `reports/2026/report.txt`.
    /// The name and its path components are validated for the target platform.
    /// A separate transfer-level check rejects duplicate or conflicting paths.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] when the
    /// name is empty or invalid.
    pub fn new(name: &str, source: Box<dyn FileSource>) -> Result<Self> {
        validate_file_name(name)?;

        Ok(Self {
            name: name.to_string(),
            size: None,
            source,
        })
    }

    /// Returns the virtual file's name or relative destination path.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the explicitly supplied size, if any.
    ///
    /// `None` does not imply an empty file. A seekable source may still provide
    /// its length when opened by the receiving application.
    pub fn size(&self) -> Option<u64> {
        self.size
    }

    /// Sets the advertised size in bytes and returns the updated file.
    ///
    /// This does not inspect the source. The caller is responsible for ensuring
    /// the declared size matches the bytes the source provides.
    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    pub(crate) fn open(&self) -> io::Result<SourceReader> {
        self.source.open()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, InputError};
    use std::io::{self, Read};
    struct StubSource;

    impl FileSource for StubSource {
        fn open(&self) -> io::Result<SourceReader> {
            Ok(SourceReader::seekable(std::io::Cursor::new(
                b"hello".to_vec(),
            )))
        }
    }

    #[test]
    fn creates_virtual_file() {
        let file = VirtualFile::new("hello.txt", Box::new(StubSource)).unwrap();

        assert_eq!(file.name(), "hello.txt");
        assert_eq!(file.size(), None);
    }

    #[test]
    fn reads_from_file_source() {
        let file = VirtualFile::new("hello.txt", Box::new(StubSource)).unwrap();

        let mut reader = file.open().unwrap();

        let mut buffer = [0u8; 5];
        let count = reader.read(&mut buffer).unwrap();

        assert_eq!(count, 5);
        assert_eq!(&buffer, b"hello");
    }

    #[test]
    fn rejects_empty_name() {
        let result = VirtualFile::new("", Box::new(StubSource));

        assert!(matches!(
            result,
            Err(Error::InvalidInput(InputError::EmptyName))
        ));
    }
}
