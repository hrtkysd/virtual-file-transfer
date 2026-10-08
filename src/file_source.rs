use crate::source_reader::SourceReader;
use std::io;

pub trait FileSource {
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
