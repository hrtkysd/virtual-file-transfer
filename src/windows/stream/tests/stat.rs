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
