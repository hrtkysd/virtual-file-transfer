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
