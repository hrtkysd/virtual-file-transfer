use super::*;

#[test]
fn seekable_reader_can_seek() {
    let mut reader = SourceReader::seekable(Cursor::new(b"hello".to_vec()));

    assert_eq!(reader.seek(SeekFrom::Start(2)).unwrap(), 2);

    let mut buffer = [0u8; 2];
    reader.read_exact(&mut buffer).unwrap();

    assert_eq!(&buffer, b"ll");
}

#[test]
fn com_stream_can_seek() {
    let reader = SourceReader::seekable(Cursor::new(b"hello".to_vec()));

    let stream: IStream = Stream::new(reader, None, CancellationToken::new(), None).into();

    let mut position = 0u64;

    unsafe {
        stream
            .Seek(2, STREAM_SEEK_SET, Some(&mut position))
            .unwrap();
    }

    assert_eq!(position, 2);

    let sequential: &ISequentialStream = (&stream).into();

    let mut buffer = [0u8; 2];
    let mut read = 0;

    let hr = unsafe {
        sequential.Read(
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            Some(&mut read),
        )
    };

    assert_eq!(hr, S_OK);
    assert_eq!(&buffer, b"ll");
}

#[test]
fn com_stream_seek_fails_for_streaming_reader() {
    let reader = SourceReader::streaming(Cursor::new(b"hello".to_vec()));

    let stream: IStream = Stream::new(reader, Some(5), CancellationToken::new(), None).into();

    let error = unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.unwrap_err();

    assert_eq!(error.code(), STG_E_INVALIDFUNCTION);
}

#[test]
fn com_stream_can_seek_across_4gib_boundary() {
    const SIZE: u64 = FOUR_GIB + 2;

    let reader = SourceReader::seekable(LargeReader::new(SIZE));
    let stream: IStream = Stream::new(reader, None, CancellationToken::new(), None).into();

    let mut position = 0u64;

    // Seek to one byte before the 4 GiB boundary.
    unsafe {
        stream
            .Seek((FOUR_GIB - 1) as i64, STREAM_SEEK_SET, Some(&mut position))
            .unwrap();
    }
    assert_eq!(position, FOUR_GIB - 1);

    // Read across the boundary.
    let sequential: &ISequentialStream = (&stream).into();
    let mut buffer = [0u8; 3];
    let mut bytes_read = 0u32;

    let hr = unsafe {
        sequential.Read(
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            Some(&mut bytes_read),
        )
    };

    assert_eq!(hr, S_OK);
    assert_eq!(bytes_read, 3);
    assert_eq!(buffer, [255, 0, 1]);

    // Seek backwards relative to the current position.
    unsafe {
        stream
            .Seek(-2, STREAM_SEEK_CUR, Some(&mut position))
            .unwrap();
    }
    assert_eq!(position, FOUR_GIB);

    // Seek relative to the end.
    unsafe {
        stream
            .Seek(-1, STREAM_SEEK_END, Some(&mut position))
            .unwrap();
    }
    assert_eq!(position, SIZE - 1);
}
