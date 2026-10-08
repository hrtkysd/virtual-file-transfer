use crate::file_source::FileSource;
use crate::source_reader::SourceReader;
use crate::{Error, InputError, Result};
use std::io;

pub struct VirtualFile {
    name: String,
    size: Option<u64>,
    source: Box<dyn FileSource>,
}

impl VirtualFile {
    pub fn new(name: &str, source: Box<dyn FileSource>) -> Result<Self> {
        if name.is_empty() {
            return Err(Error::InvalidInput(InputError::EmptyName));
        }

        Ok(Self {
            name: name.to_string(),
            size: None,
            source,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn size(&self) -> Option<u64> {
        self.size
    }

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
