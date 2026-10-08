use std::io::{self, Read, Seek, SeekFrom};

pub trait ReadSeek: Read + Seek {}

impl<T> ReadSeek for T where T: Read + Seek {}

pub enum SourceReader {
    Streaming(Box<dyn Read>),
    Seekable(Box<dyn ReadSeek>),
}

#[derive(Debug)]
pub(crate) enum SourceReaderError {
    NotSeekable,
    Io(io::Error),
}

impl SourceReader {
    pub fn streaming<R>(reader: R) -> Self
    where
        R: Read + 'static,
    {
        Self::Streaming(Box::new(reader))
    }

    pub fn seekable<R>(reader: R) -> Self
    where
        R: Read + Seek + 'static,
    {
        Self::Seekable(Box::new(reader))
    }

    pub(crate) fn seek(&mut self, position: std::io::SeekFrom) -> Result<u64, SourceReaderError> {
        match self {
            Self::Seekable(reader) => reader.seek(position).map_err(SourceReaderError::Io),

            Self::Streaming(_) => Err(SourceReaderError::NotSeekable),
        }
    }

    pub(crate) fn try_size(&mut self) -> io::Result<Option<u64>> {
        match self {
            Self::Streaming(_) => Ok(None),

            Self::Seekable(reader) => {
                let current = reader.seek(SeekFrom::Current(0))?;
                let size = reader.seek(SeekFrom::End(0))?;
                reader.seek(SeekFrom::Start(current))?;

                Ok(Some(size))
            }
        }
    }
}

impl Read for SourceReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Streaming(reader) => reader.read(buffer),
            Self::Seekable(reader) => reader.read(buffer),
        }
    }
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
}
