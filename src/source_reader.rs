use std::io::{self, Read, Seek, SeekFrom};

use crate::source_reader::ReaderKind::{Seekable, Streaming};

pub struct SourceReader {
    inner: ReaderKind,
}

pub trait ReadSeek: Read + Seek {}

impl<T> ReadSeek for T where T: Read + Seek {}

enum ReaderKind {
    Streaming(Box<dyn Read + Send>),
    Seekable(Box<dyn ReadSeek + Send>),
}

#[derive(Debug)]
pub(crate) enum SourceReaderError {
    NotSeekable,
    Io,
}

impl SourceReader {
    pub fn streaming<R>(reader: R) -> Self
    where
        R: Read + Send + 'static,
    {
        Self {
            inner: Streaming(Box::new(reader)),
        }
    }

    pub fn seekable<R>(reader: R) -> Self
    where
        R: Read + Seek + Send + 'static,
    {
        Self {
            inner: Seekable(Box::new(reader)),
        }
    }

    #[cfg(feature = "spool")]
    pub fn spooled<R>(reader: R) -> io::Result<Self>
    where
        R: Read + Send + 'static,
    {
        Ok(Self::seekable(spool_to_file(reader)?))
    }

    pub(crate) fn seek(&mut self, position: SeekFrom) -> Result<u64, SourceReaderError> {
        match &mut self.inner {
            ReaderKind::Seekable(reader) => {
                reader.seek(position).map_err(|_| SourceReaderError::Io)
            }

            ReaderKind::Streaming(_) => Err(SourceReaderError::NotSeekable),
        }
    }

    pub(crate) fn try_size(&mut self) -> io::Result<Option<u64>> {
        match &mut self.inner {
            ReaderKind::Streaming(_) => Ok(None),

            ReaderKind::Seekable(reader) => {
                let current = reader.stream_position()?;
                let size = reader.seek(SeekFrom::End(0))?;
                reader.seek(SeekFrom::Start(current))?;

                Ok(Some(size))
            }
        }
    }
}

impl Read for SourceReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            ReaderKind::Streaming(reader) => reader.read(buffer),
            ReaderKind::Seekable(reader) => reader.read(buffer),
        }
    }
}

#[cfg(feature = "spool")]
fn spool_to_file(mut reader: impl Read) -> io::Result<std::fs::File> {
    let mut file = tempfile::tempfile()?;

    io::copy(&mut reader, &mut file)?;
    file.seek(SeekFrom::Start(0))?;

    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_reader_is_not_seekable() {
        let mut reader = SourceReader::streaming(std::io::Cursor::new(b"hello".to_vec()));

        assert!(matches!(
            reader.seek(SeekFrom::Start(0)),
            Err(SourceReaderError::NotSeekable)
        ));
    }

    #[test]
    fn streaming_reader_cannot_derive_size() {
        let mut reader = SourceReader::streaming(std::io::Cursor::new(b"hello".to_vec()));

        assert_eq!(reader.try_size().unwrap(), None);
    }

    #[test]
    fn seekable_reader_can_seek() {
        let mut reader = SourceReader::seekable(std::io::Cursor::new(b"hello".to_vec()));

        assert_eq!(reader.seek(SeekFrom::Start(2)).unwrap(), 2);

        let mut buf = [0u8; 2];
        reader.read_exact(&mut buf).unwrap();

        assert_eq!(&buf, b"ll");
    }

    #[test]
    fn seekable_reader_can_derive_size() {
        let mut reader = SourceReader::seekable(std::io::Cursor::new(b"hello".to_vec()));

        assert_eq!(reader.try_size().unwrap(), Some(5));
    }

    #[test]
    fn try_size_restores_position() {
        let mut reader = SourceReader::seekable(std::io::Cursor::new(b"hello".to_vec()));

        reader.seek(SeekFrom::Start(2)).unwrap();

        assert_eq!(reader.try_size().unwrap(), Some(5));
        assert_eq!(reader.seek(SeekFrom::Current(0)).unwrap(), 2);
    }

    #[cfg(feature = "spool")]
    #[test]
    fn spooled_reader_preserves_data_and_supports_seek() {
        let data: Vec<u8> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();

        let mut reader = SourceReader::spooled(io::Cursor::new(data.clone())).unwrap();

        // Size can be derived.
        assert_eq!(reader.try_size().unwrap(), Some(data.len() as u64));

        // Reading starts at position zero.
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, data);

        // Random access is supported.
        assert_eq!(reader.seek(SeekFrom::Start(42)).unwrap(), 42);

        // Size lookup preserves the current position.
        assert_eq!(reader.try_size().unwrap(), Some(data.len() as u64));
        assert_eq!(reader.seek(SeekFrom::Current(0)).unwrap(), 42);

        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, &data[42..46]);
    }

    #[cfg(feature = "spool")]
    #[test]
    fn spooled_reader_accepts_empty_source() {
        let mut reader = SourceReader::spooled(io::empty()).unwrap();

        assert_eq!(reader.try_size().unwrap(), Some(0));

        let mut buf = [0u8; 1];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[cfg(feature = "spool")]
    #[test]
    fn spooled_reader_propagates_source_error() {
        struct FailingReader;

        impl Read for FailingReader {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("source read failed"))
            }
        }

        let result = SourceReader::spooled(FailingReader);

        assert!(matches!(
            result,
            Err(ref err) if err.kind() == io::ErrorKind::Other
        ));
    }

    #[cfg(feature = "spool")]
    #[test]
    fn spool_creates_file_with_correct_contents() {
        let data = b"hello spool";

        let mut file = spool_to_file(io::Cursor::new(data)).unwrap();

        assert_eq!(file.metadata().unwrap().len(), data.len() as u64);

        let mut actual = Vec::new();
        file.read_to_end(&mut actual).unwrap();

        assert_eq!(actual, data);

        drop(file);
    }

    #[cfg(feature = "spool")]
    #[test]
    fn spooled_reader_supports_unknown_size_non_seekable_source() {
        const SIZE: u64 = 1024 * 1024 + 7;

        let mut source = SourceReader::streaming(io::repeat(0x5A).take(SIZE));

        assert_eq!(source.try_size().unwrap(), None);
        assert!(matches!(
            source.seek(SeekFrom::Start(0)),
            Err(SourceReaderError::NotSeekable)
        ));

        let mut reader = SourceReader::spooled(source).unwrap();

        assert_eq!(reader.try_size().unwrap(), Some(SIZE));

        assert_eq!(reader.seek(SeekFrom::End(-1)).unwrap(), SIZE - 1);

        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 0x5A);

        assert_eq!(reader.read(&mut buf).unwrap(), 0);

        assert_eq!(reader.seek(SeekFrom::Start(0)).unwrap(), 0);
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 0x5A);
    }
}
