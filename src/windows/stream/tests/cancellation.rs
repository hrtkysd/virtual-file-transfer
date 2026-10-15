use super::*;

#[test]
fn read_aborts_when_cancelled_during_read() {
    let token = CancellationToken::new();
    let read_count = Arc::new(AtomicUsize::new(0));

    let reader = SourceReader::seekable(CancelOnFirstRead {
        inner: Cursor::new(b"hello".to_vec()),
        token: token.clone(),
        read_count: read_count.clone(),
    });

    let stream: IStream = Stream::new(reader, Some(5), token, None).into();
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

    assert_eq!(hr, E_ABORT);
    assert_eq!(read, 2);
    assert_eq!(&buffer[..2], b"he");
    assert_eq!(read_count.load(Ordering::Relaxed), 1);

    // Partial reads must still update the stream position.
    let mut position = 0u64;
    unsafe {
        stream
            .Seek(0, STREAM_SEEK_CUR, Some(&mut position))
            .unwrap();
    }
    assert_eq!(position, 2);

    // Further reads must fail without accessing the source.
    let mut read = u32::MAX;

    let hr = unsafe {
        sequential.Read(
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            Some(&mut read),
        )
    };

    assert_eq!(hr, E_ABORT);
    assert_eq!(read, 0);
    assert_eq!(read_count.load(Ordering::Relaxed), 1);
}

#[test]
fn read_returns_abort_when_cancelled_before_read() {
    let token = CancellationToken::new();
    let reader = SourceReader::streaming(Cursor::new(b"hello".to_vec()));

    let stream: IStream = Stream::new(reader, Some(5), token.clone(), None).into();
    let sequential: &ISequentialStream = (&stream).into();

    token.cancel();

    let mut buffer = [0u8; 5];
    let mut read = u32::MAX;

    let hr = unsafe {
        sequential.Read(
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            Some(&mut read),
        )
    };

    assert_eq!(hr, E_ABORT);
    assert_eq!(read, 0);
    assert_eq!(buffer, [0; 5]);
}

#[test]
fn cancellation_is_shared_between_streams() {
    let token = CancellationToken::new();

    let stream_a: IStream = Stream::new(
        SourceReader::streaming(Cursor::new(b"aaa".to_vec())),
        Some(3),
        token.clone(),
        None,
    )
    .into();

    let stream_b: IStream = Stream::new(
        SourceReader::streaming(Cursor::new(b"bbb".to_vec())),
        Some(3),
        token.clone(),
        None,
    )
    .into();

    token.cancel();

    for stream in [&stream_a, &stream_b] {
        let sequential: &ISequentialStream = stream.into();

        let mut buffer = [0u8; 3];
        let mut read = u32::MAX;

        let hr = unsafe {
            sequential.Read(
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                Some(&mut read),
            )
        };

        assert_eq!(hr, E_ABORT);
        assert_eq!(read, 0);
    }
}
