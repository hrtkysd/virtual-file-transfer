use crate::source_reader::SourceReader;
use std::io;

/// A provider of virtual file contents.
///
/// Each call to [`FileSource::open`] must create a fresh,
/// independent reader positioned at the beginning of the
/// file.
///
/// A receiving application may request the contents of
/// the same virtual file multiple times or concurrently.
///
/// Implementations must be safe to share across threads.
pub trait FileSource: Send + Sync {
    /// Opens a new reader for this file's contents.
    ///
    /// The returned reader must not depend on the position of readers returned
    /// by earlier calls.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the contents cannot be opened or prepared.
    fn open(&self) -> io::Result<SourceReader>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    struct MemorySource {
        data: Vec<u8>,
    }

    impl FileSource for MemorySource {
        fn open(&self) -> io::Result<SourceReader> {
            Ok(SourceReader::seekable(std::io::Cursor::new(
                self.data.clone(),
            )))
        }
    }

    #[test]
    fn reads_data() {
        let source = MemorySource {
            data: b"hello".to_vec(),
        };

        let mut reader = source.open().unwrap();
        let mut buffer = [0u8; 3];
        let count = reader.read(&mut buffer).unwrap();

        assert_eq!(count, 3);
        assert_eq!(&buffer, b"hel");
    }
}
