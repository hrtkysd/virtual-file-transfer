use super::*;

#[test]
fn stat_uses_known_size_for_streaming_reader() {
    let reader = SourceReader::streaming(Cursor::new(b"hello".to_vec()));

    let stream: IStream = Stream::new(reader, Some(5), CancellationToken::new(), None).into();
    let mut stat = STATSTG::default();

    unsafe {
        stream.Stat(&mut stat, STATFLAG_DEFAULT).unwrap();
    }

    assert_eq!(stat.cbSize, 5);
}

#[test]
fn stat_fails_for_unknown_size_streaming_reader() {
    let reader = SourceReader::streaming(std::io::Cursor::new(b"hello".to_vec()));

    let stream: IStream = Stream::new(reader, None, CancellationToken::new(), None).into();
    let mut stat = STATSTG::default();

    let error = unsafe { stream.Stat(&mut stat, STATFLAG_DEFAULT) }.unwrap_err();

    assert_eq!(error.code(), STG_E_INVALIDFUNCTION);
}

#[test]
fn stat_derives_size_for_unknown_seekable_reader() {
    let reader = SourceReader::seekable(Cursor::new(b"hello".to_vec()));

    let stream: IStream = Stream::new(reader, None, CancellationToken::new(), None).into();
    let mut stat = STATSTG::default();

    unsafe {
        stream.Stat(&mut stat, STATFLAG_DEFAULT).unwrap();
    }

    assert_eq!(stat.cbSize, 5);
}

#[test]
fn stat_reports_large_known_size() {
    const SIZE: u64 = FOUR_GIB + 1;

    let reader = SourceReader::streaming(io::repeat(0));
    let stream: IStream = Stream::new(reader, Some(SIZE), CancellationToken::new(), None).into();

    let mut stat = STATSTG::default();

    unsafe {
        stream.Stat(&mut stat, STATFLAG_DEFAULT).unwrap();
    }

    assert_eq!(stat.cbSize, SIZE);
}

#[test]
fn stat_derives_large_size_without_changing_position() {
    const SIZE: u64 = FOUR_GIB + 2;

    let reader = SourceReader::seekable(LargeReader::new(SIZE));
    let stream: IStream = Stream::new(reader, None, CancellationToken::new(), None).into();

    // Move the stream close to the 4 GiB boundary.
    unsafe {
        stream
            .Seek((FOUR_GIB - 1) as i64, STREAM_SEEK_SET, None)
            .unwrap();
    }

    // Stat must derive the size without altering the position.
    let mut stat = STATSTG::default();
    unsafe {
        stream.Stat(&mut stat, STATFLAG_DEFAULT).unwrap();
    }

    assert_eq!(stat.cbSize, SIZE);

    let mut position = 0u64;
    unsafe {
        stream
            .Seek(0, STREAM_SEEK_CUR, Some(&mut position))
            .unwrap();
    }

    assert_eq!(position, FOUR_GIB - 1);

    // Verify the next read still starts at the original position.
    let sequential: &ISequentialStream = (&stream).into();
    let mut buffer = [0u8; 1];
    let mut bytes_read = 0u32;

    let hr = unsafe { sequential.Read(buffer.as_mut_ptr().cast(), 1, Some(&mut bytes_read)) };

    assert_eq!(hr, S_OK);
    assert_eq!(bytes_read, 1);
    assert_eq!(buffer, [255]);
}
