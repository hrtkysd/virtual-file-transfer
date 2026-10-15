use super::*;

#[implement(IStream)]
struct PartialWriteDestination {
    bytes: Arc<Mutex<Vec<u8>>>,
    max_write: u32,
}

#[allow(non_snake_case)]
impl ISequentialStream_Impl for PartialWriteDestination_Impl {
    fn Read(&self, _pv: *mut c_void, _cb: u32, _pcbread: *mut u32) -> HRESULT {
        STG_E_ACCESSDENIED
    }

    fn Write(&self, pv: *const c_void, cb: u32, pcbwritten: *mut u32) -> HRESULT {
        if !pcbwritten.is_null() {
            unsafe { pcbwritten.write(0) };
        }

        if cb > 0 && pv.is_null() {
            return STG_E_INVALIDPOINTER;
        }

        let count = cb.min(self.max_write);

        if count > 0 {
            let data = unsafe { std::slice::from_raw_parts(pv.cast::<u8>(), count as usize) };

            self.bytes.lock().unwrap().extend_from_slice(data);
        }

        if !pcbwritten.is_null() {
            unsafe { pcbwritten.write(count) };
        }

        S_OK
    }
}

#[allow(non_snake_case)]
impl IStream_Impl for PartialWriteDestination_Impl {
    fn Seek(
        &self,
        _dlibmove: i64,
        _dworigin: STREAM_SEEK,
        _plibnewposition: *mut u64,
    ) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn SetSize(&self, _libnewsize: u64) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn CopyTo(
        &self,
        _pstm: Ref<'_, IStream>,
        _cb: u64,
        _pcbread: *mut u64,
        _pcbwritten: *mut u64,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Commit(&self, _grfcommitflags: &STGC) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn Revert(&self) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn LockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: &LOCKTYPE) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn UnlockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: u32) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn Stat(&self, _pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn Clone(&self) -> Result<IStream> {
        Err(E_NOTIMPL.into())
    }
}

struct FailAfterFirstRead {
    inner: Cursor<Vec<u8>>,
    read_calls: usize,
}

impl std::io::Read for FailAfterFirstRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.read_calls > 0 {
            return Err(std::io::Error::other("Simulated read failure"));
        }

        self.read_calls += 1;
        std::io::Read::read(&mut self.inner, buf)
    }
}

fn make_source(data: &[u8], token: CancellationToken) -> IStream {
    Stream::new(
        SourceReader::seekable(Cursor::new(data.to_vec())),
        Some(data.len() as u64),
        token,
        None,
    )
    .into()
}
fn make_destination() -> IStream {
    unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true).unwrap() }
}

fn read_destination(stream: &IStream, size: usize) -> Vec<u8> {
    unsafe {
        stream.Seek(0, STREAM_SEEK_SET, None).unwrap();
    }

    let mut buffer = vec![0u8; size];
    let mut bytes_read = 0u32;

    let hr = unsafe {
        stream.Read(
            buffer.as_mut_ptr().cast(),
            size as u32,
            Some(&mut bytes_read),
        )
    };

    assert_eq!(hr, S_OK);
    assert_eq!(bytes_read as usize, size);

    buffer
}

#[test]
fn copy_to_copies_requested_bytes() {
    let source = make_source(b"hello world", CancellationToken::new());
    let destination = make_destination();

    let mut read = 0u64;
    let mut written = 0u64;

    unsafe {
        source
            .CopyTo(&destination, 5, Some(&mut read), Some(&mut written))
            .unwrap();
    }

    assert_eq!(read, 5);
    assert_eq!(written, 5);
    assert_eq!(read_destination(&destination, 5), b"hello");

    // The source position must advance by the copied byte count.
    let mut position = 0u64;
    unsafe {
        source
            .Seek(0, STREAM_SEEK_CUR, Some(&mut position))
            .unwrap();
    }
    assert_eq!(position, 5);
}

#[test]
fn copy_to_stops_at_eof() {
    let source = make_source(b"hello", CancellationToken::new());
    let destination = make_destination();

    let mut read = 0u64;
    let mut written = 0u64;

    unsafe {
        source
            .CopyTo(&destination, 100, Some(&mut read), Some(&mut written))
            .unwrap();
    }

    assert_eq!(read, 5);
    assert_eq!(written, 5);
    assert_eq!(read_destination(&destination, 5), b"hello");
}

#[test]
fn copy_to_handles_multiple_chunks() {
    const SIZE: usize = 64 * 1024 + 17;

    let data: Vec<u8> = (0..SIZE).map(|i| (i % 251) as u8).collect();

    let source = make_source(&data, CancellationToken::new());
    let destination = make_destination();

    let mut read = 0u64;
    let mut written = 0u64;

    unsafe {
        source
            .CopyTo(
                &destination,
                SIZE as u64,
                Some(&mut read),
                Some(&mut written),
            )
            .unwrap();
    }

    assert_eq!(read, SIZE as u64);
    assert_eq!(written, SIZE as u64);
    assert_eq!(read_destination(&destination, SIZE), data);
}

#[test]
fn copy_to_accepts_null_output_pointers() {
    let source = make_source(b"hello", CancellationToken::new());
    let destination = make_destination();

    unsafe {
        source.CopyTo(&destination, 5, None, None).unwrap();
    }

    assert_eq!(read_destination(&destination, 5), b"hello");
}

#[test]
fn copy_to_propagates_write_failure() {
    let source = make_source(b"hello", CancellationToken::new());

    // Our Stream is read-only and rejects Write().
    let destination = make_source(b"", CancellationToken::new());

    let mut read = 0u64;
    let mut written = 0u64;

    let error =
        unsafe { source.CopyTo(&destination, 5, Some(&mut read), Some(&mut written)) }.unwrap_err();

    assert_eq!(error.code(), STG_E_ACCESSDENIED);
    assert_eq!(read, 5);
    assert_eq!(written, 0);
}

#[test]
fn copy_to_respects_cancellation() {
    let token = CancellationToken::new();
    let read_count = Arc::new(AtomicUsize::new(0));

    let reader = SourceReader::seekable(CancelOnFirstRead {
        inner: Cursor::new(b"hello".to_vec()),
        token: token.clone(),
        read_count: read_count.clone(),
    });

    let source: IStream = Stream::new(reader, Some(5), token, None).into();

    let destination = make_destination();

    let mut read = 0u64;
    let mut written = 0u64;

    let error =
        unsafe { source.CopyTo(&destination, 5, Some(&mut read), Some(&mut written)) }.unwrap_err();

    assert_eq!(error.code(), E_ABORT);
    assert_eq!(read_count.load(Ordering::Relaxed), 1);

    // The reader returned 2 bytes before cancellation.
    assert_eq!(read, 2);
    assert_eq!(written, 0);
}

#[test]
fn copy_to_handles_partial_writes() {
    let source = make_source(b"hello", CancellationToken::new());

    let bytes = Arc::new(Mutex::new(Vec::new()));

    let destination: IStream = PartialWriteDestination {
        bytes: Arc::clone(&bytes),
        max_write: 2,
    }
    .into();

    let mut read = 0u64;
    let mut written = 0u64;

    unsafe {
        source
            .CopyTo(&destination, 5, Some(&mut read), Some(&mut written))
            .unwrap();
    }

    assert_eq!(read, 5);
    assert_eq!(written, 5);
    assert_eq!(&*bytes.lock().unwrap(), b"hello");
}

#[test]
fn copy_to_zero_bytes() {
    let source = make_source(b"hello", CancellationToken::new());
    let destination = make_destination();

    let mut read = u64::MAX;
    let mut written = u64::MAX;

    unsafe {
        source
            .CopyTo(&destination, 0, Some(&mut read), Some(&mut written))
            .unwrap();
    }

    assert_eq!(read, 0);
    assert_eq!(written, 0);

    let mut position = u64::MAX;
    unsafe {
        source
            .Seek(0, STREAM_SEEK_CUR, Some(&mut position))
            .unwrap();
    }

    assert_eq!(position, 0);
}

#[test]
fn copy_to_rejects_zero_progress_write() {
    let source = make_source(b"hello", CancellationToken::new());

    let bytes = Arc::new(Mutex::new(Vec::new()));

    let destination: IStream = PartialWriteDestination {
        bytes: Arc::clone(&bytes),
        max_write: 0,
    }
    .into();

    let mut read = 0u64;
    let mut written = 0u64;

    let error =
        unsafe { source.CopyTo(&destination, 5, Some(&mut read), Some(&mut written)) }.unwrap_err();

    assert_eq!(error.code(), STG_E_WRITEFAULT);
    assert_eq!(read, 5);
    assert_eq!(written, 0);
    assert!(bytes.lock().unwrap().is_empty());
}

#[test]
fn copy_to_propagates_read_failure() {
    const CHUNK_SIZE: usize = 64 * 1024;
    const SIZE: usize = CHUNK_SIZE + 1;

    let data = vec![0xAB; SIZE];

    let reader = FailAfterFirstRead {
        inner: Cursor::new(data),
        read_calls: 0,
    };

    let source: IStream = Stream::new(
        SourceReader::streaming(reader),
        Some(SIZE as u64),
        CancellationToken::new(),
        None,
    )
    .into();

    let destination = make_destination();

    let mut read = 0u64;
    let mut written = 0u64;

    let error = unsafe {
        source.CopyTo(
            &destination,
            SIZE as u64,
            Some(&mut read),
            Some(&mut written),
        )
    }
    .unwrap_err();

    assert_eq!(error.code(), STG_E_READFAULT);

    // The first chunk was transferred before the read failure.
    assert_eq!(read, CHUNK_SIZE as u64);
    assert_eq!(written, CHUNK_SIZE as u64);

    assert_eq!(
        read_destination(&destination, CHUNK_SIZE),
        vec![0xAB; CHUNK_SIZE]
    );
}
