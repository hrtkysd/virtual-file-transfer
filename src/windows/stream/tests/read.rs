use super::*;

#[test]
fn reads_from_com_stream() {
    let reader = SourceReader::seekable(std::io::Cursor::new(b"hello".to_vec()));
    let stream: IStream = Stream::new(reader, Some(5), CancellationToken::new(), None).into();
    let sequential: &ISequentialStream = (&stream).into();

    {
        let mut buffer = [0u8; 3];
        let mut read = 0u32;

        let hr = unsafe {
            sequential.Read(
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                Some(&mut read),
            )
        };

        assert_eq!(hr, S_OK);
        assert_eq!(read, 3);
        assert_eq!(&buffer, b"hel");
    }
    {
        let mut buffer = [0u8; 2];
        let mut read = 0u32;

        let hr = unsafe {
            sequential.Read(
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                Some(&mut read),
            )
        };

        assert_eq!(hr, S_OK);
        assert_eq!(read, 2);
        assert_eq!(&buffer, b"lo");
    }
}

#[test]
fn read_fails_when_source_ends_before_declared_size() {
    let reader = SourceReader::streaming(Cursor::new(b"hello".to_vec()));

    let stream: IStream = Stream::new(reader, Some(10), CancellationToken::new(), None).into();
    let sequential: &ISequentialStream = (&stream).into();

    let mut buffer = [0u8; 10];
    let mut read = 0u32;

    let hr = unsafe {
        sequential.Read(
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            Some(&mut read),
        )
    };

    assert_eq!(hr, STG_E_READFAULT);
    assert_eq!(read, 5);
}

#[test]
fn read_tracks_position_across_calls() {
    let reader = SourceReader::streaming(Cursor::new(b"hello".to_vec()));

    let stream: IStream = Stream::new(reader, Some(10), CancellationToken::new(), None).into();
    let sequential: &ISequentialStream = (&stream).into();

    let mut first = [0u8; 3];

    let hr = unsafe { sequential.Read(first.as_mut_ptr().cast(), 3, None) };

    assert_eq!(hr, S_OK);

    let mut second = [0u8; 7];
    let mut read = 0u32;

    let hr = unsafe { sequential.Read(second.as_mut_ptr().cast(), 7, Some(&mut read)) };

    assert_eq!(hr, STG_E_READFAULT);
    assert_eq!(read, 2);
}

#[test]
fn read_collects_short_source_reads() {
    struct ShortReader {
        inner: Cursor<Vec<u8>>,
    }

    impl Read for ShortReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let len = buf.len().min(2);
            self.inner.read(&mut buf[..len])
        }
    }

    let reader = SourceReader::streaming(ShortReader {
        inner: Cursor::new(b"hello".to_vec()),
    });

    let stream: IStream = Stream::new(reader, Some(5), CancellationToken::new(), None).into();

    let sequential: &ISequentialStream = (&stream).into();
    let mut buffer = [0u8; 5];
    let mut read = 0u32;

    let hr = unsafe {
        sequential.Read(
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            Some(&mut read),
        )
    };

    assert_eq!(hr, S_OK);
    assert_eq!(read, 5);
    assert_eq!(&buffer, b"hello");
}

#[test]
fn read_retries_interrupted_source() {
    struct InterruptedOnce {
        inner: Cursor<Vec<u8>>,
        interrupted: bool,
    }

    impl Read for InterruptedOnce {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }

            self.inner.read(buf)
        }
    }

    let reader = SourceReader::streaming(InterruptedOnce {
        inner: Cursor::new(b"hello".to_vec()),
        interrupted: false,
    });

    let stream: IStream = Stream::new(reader, Some(5), CancellationToken::new(), None).into();

    let sequential: &ISequentialStream = (&stream).into();
    let mut buffer = [0u8; 5];
    let mut read = 0u32;

    let hr = unsafe {
        sequential.Read(
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            Some(&mut read),
        )
    };

    assert_eq!(hr, S_OK);
    assert_eq!(read, 5);
    assert_eq!(&buffer, b"hello");
}

#[test]
fn read_preserves_partial_count_and_position_on_io_error() {
    struct FailAfterFirstChunk {
        inner: Cursor<Vec<u8>>,
        first_read: bool,
    }

    impl Read for FailAfterFirstChunk {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.first_read {
                return Err(io::Error::other("simulated read failure"));
            }

            self.first_read = false;

            let len = buf.len().min(2);
            self.inner.read(&mut buf[..len])
        }
    }

    impl Seek for FailAfterFirstChunk {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    let reader = SourceReader::seekable(FailAfterFirstChunk {
        inner: Cursor::new(b"hello".to_vec()),
        first_read: true,
    });

    let stream: IStream = Stream::new(reader, Some(5), CancellationToken::new(), None).into();

    let sequential: &ISequentialStream = (&stream).into();

    let mut buffer = [0u8; 5];
    let mut read = 0u32;

    let hr = unsafe {
        sequential.Read(
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            Some(&mut read),
        )
    };

    // 2 bytes were read before the source failed.
    assert_eq!(hr, STG_E_READFAULT);
    assert_eq!(read, 2);
    assert_eq!(&buffer[..2], b"he");

    // The stream position must reflect the partial read.
    let mut position = 0u64;

    unsafe {
        stream
            .Seek(0, STREAM_SEEK_CUR, Some(&mut position))
            .unwrap();
    }

    assert_eq!(position, 2);

    // Another read must report the source error.
    let mut read = u32::MAX;

    let hr = unsafe {
        sequential.Read(
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            Some(&mut read),
        )
    };

    assert_eq!(hr, STG_E_READFAULT);
    assert_eq!(read, 0);
}
