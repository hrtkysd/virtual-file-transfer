use crate::{FileSource, SourceReader};
use std::io;

/// Adapts a reader-producing closure into a [`FileSource`].
///
/// The closure is called whenever the receiving application requests a new
/// stream. It must create an independent reader on every invocation.
///
/// Use a closure that implements `Fn() -> io::Result<SourceReader>` and is
/// `Send + Sync`.
pub struct FactorySource<F> {
    open: F,
}

impl<F> FactorySource<F>
where
    F: Fn() -> io::Result<SourceReader> + Send + Sync,
{
    /// Creates a file source using the supplied reader factory.
    ///
    /// The factory is evaluated lazily when [`FileSource::open`] is called,
    /// rather than when this value is constructed.
    pub fn new(open: F) -> Self {
        Self { open }
    }
}

impl<F> FileSource for FactorySource<F>
where
    F: Fn() -> io::Result<SourceReader> + Send + Sync,
{
    fn open(&self) -> io::Result<SourceReader> {
        (self.open)()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::io::Read;

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
