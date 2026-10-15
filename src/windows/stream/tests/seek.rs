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
