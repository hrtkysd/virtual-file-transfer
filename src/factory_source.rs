use crate::{FileSource, SourceReader};
use std::io::{self, Read};

pub struct FactorySource<F> {
    open: F,
}

impl<F> FactorySource<F> {
    pub fn new(open: F) -> Self {
        Self { open }
    }
}

impl<F> FileSource for FactorySource<F>
where
    F: Fn() -> io::Result<SourceReader>,
{
    fn open(&self) -> io::Result<SourceReader> {
        (self.open)()
    }
}
#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn factory_source_opens_reader() {
        let source = FactorySource::new(|| {
            Ok(SourceReader::seekable(std::io::Cursor::new(
                b"hello".to_vec(),
            )))
        });

        let mut reader = source.open().unwrap();

        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).unwrap();

        assert_eq!(buffer, b"hello");
    }

    #[test]
    fn factory_source_creates_independent_readers() {
        let source = FactorySource::new(|| {
            Ok(SourceReader::seekable(std::io::Cursor::new(
                b"hello".to_vec(),
            )))
        });

        let mut first = source.open().unwrap();
        let mut second = source.open().unwrap();

        let mut buffer = [0u8; 1];

        first.read_exact(&mut buffer).unwrap();
        assert_eq!(&buffer, b"h");

        second.read_exact(&mut buffer).unwrap();
        assert_eq!(&buffer, b"h");
    }
}
