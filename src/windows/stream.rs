use crate::cancellation::CancellationToken;
use crate::source_reader::{SourceReader, SourceReaderError};
use crate::transfer::TransferCompletion;
use std::ffi::c_void;
use std::io::{Read, SeekFrom};
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{
    E_ABORT, E_NOTIMPL, S_FALSE, S_OK, STG_E_ACCESSDENIED, STG_E_INVALIDFUNCTION,
    STG_E_INVALIDPOINTER, STG_E_READFAULT, STG_E_SEEKERROR, STG_E_WRITEFAULT,
};
use windows::Win32::System::Com::{
    ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE, STATFLAG, STATSTG, STGC, STGM_READ,
    STGMEDIUM, STGMEDIUM_0, STGTY_STREAM, STREAM_SEEK, STREAM_SEEK_CUR, STREAM_SEEK_END,
    STREAM_SEEK_SET, TYMED_ISTREAM,
};
use windows::core::{HRESULT, Ref, Result, implement};

struct StreamState {
    reader: SourceReader,
    current_position: u64,
}

#[implement(IStream)]
pub(crate) struct Stream {
    size: Option<u64>,
    state: Mutex<StreamState>,
    token: CancellationToken,
    completion: Option<Arc<TransferCompletion>>,
}

impl Stream {
    pub(crate) fn new(
        reader: SourceReader,
        size: Option<u64>,
        token: CancellationToken,
        completion: Option<Arc<TransferCompletion>>,
    ) -> Self {
        Self {
            size,
            state: Mutex::new(StreamState {
                reader,
                current_position: 0,
            }),
            token,
            completion,
        }
    }
}

pub(crate) fn create_stream_medium(
    reader: SourceReader,
    size: Option<u64>,
    token: CancellationToken,
    completion: Option<Arc<TransferCompletion>>,
) -> STGMEDIUM {
    let stream: IStream = Stream::new(reader, size, token, completion).into();

    STGMEDIUM {
        tymed: TYMED_ISTREAM.0 as u32,
        u: STGMEDIUM_0 {
            pstm: ManuallyDrop::new(Some(stream)),
        },
        pUnkForRelease: ManuallyDrop::new(None),
    }
}

#[allow(non_snake_case)]
impl ISequentialStream_Impl for Stream_Impl {
    fn Read(&self, pv: *mut c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        if !pcbread.is_null() {
            unsafe { pcbread.write(0) };
        }

        if cb == 0 {
            return S_OK;
        }

        if pv.is_null() {
            return STG_E_INVALIDPOINTER;
        }

        let buffer = unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>(), cb as usize) };

        let mut total_length = 0usize;
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return STG_E_READFAULT,
        };
        let read_result: std::result::Result<(), HRESULT> = {
            loop {
                if self.token.is_cancelled() {
                    break Err(E_ABORT);
                }

                if total_length == buffer.len() {
                    break Ok(());
                }

                match state.reader.read(&mut buffer[total_length..]) {
                    Ok(0) => break Ok(()),

                    Ok(read_length) => {
                        total_length += read_length;
                    }

                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                        continue;
                    }

                    Err(_) => break Err(STG_E_READFAULT),
                }
            }
        };

        if !pcbread.is_null() {
            unsafe { pcbread.write(total_length as u32) };
        }

        let Some(read_position) = state.current_position.checked_add(total_length as u64) else {
            return STG_E_READFAULT;
        };

        state.current_position = read_position;

        let failure = match read_result {
            Err(hr) => Some(hr),
            Ok(()) if self.token.is_cancelled() => Some(E_ABORT),
            Ok(()) => None,
        };

        if let Some(hr) = failure {
            drop(state);

            if hr == E_ABORT
                && let Some(completion) = &self.completion
                && completion.mark_as_read_aborted().is_err()
            {
                return STG_E_READFAULT;
            }

            return hr;
        }

        if let Some(size) = self.size {
            if total_length > 0 && read_position > size {
                return STG_E_READFAULT;
            }

            if total_length < buffer.len() && read_position < size {
                return STG_E_READFAULT;
            }
        }

        if total_length == buffer.len() {
            S_OK
        } else {
            S_FALSE
        }
    }

    fn Write(&self, _pv: *const c_void, _cb: u32, _pcbwritten: *mut u32) -> HRESULT {
        STG_E_ACCESSDENIED
    }
}

#[allow(non_snake_case)]
impl IStream_Impl for Stream_Impl {
    fn Seek(&self, dlibmove: i64, dworigin: STREAM_SEEK, plibnewposition: *mut u64) -> Result<()> {
        let seek_from = match dworigin.0 {
            x if x == STREAM_SEEK_SET.0 => {
                if dlibmove < 0 {
                    return Err(STG_E_INVALIDFUNCTION.into());
                }

                SeekFrom::Start(dlibmove as u64)
            }

            x if x == STREAM_SEEK_CUR.0 => SeekFrom::Current(dlibmove),

            x if x == STREAM_SEEK_END.0 => SeekFrom::End(dlibmove),

            _ => {
                return Err(STG_E_INVALIDFUNCTION.into());
            }
        };
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return Err(STG_E_READFAULT.into()),
        };
        match state.reader.seek(seek_from) {
            Ok(position) => {
                state.current_position = position;
                if !plibnewposition.is_null() {
                    unsafe {
                        plibnewposition.write(position);
                    }
                }

                Ok(())
            }

            Err(SourceReaderError::NotSeekable) => Err(STG_E_INVALIDFUNCTION.into()),

            Err(SourceReaderError::Io) => Err(STG_E_SEEKERROR.into()),
        }
    }

    fn SetSize(&self, _libnewsize: u64) -> Result<()> {
        Err(STG_E_ACCESSDENIED.into())
    }

    fn CopyTo(
        &self,
        pstm: Ref<'_, IStream>,
        cb: u64,
        pcbread: *mut u64,
        pcbwritten: *mut u64,
    ) -> Result<()> {
        const BUFFER_SIZE: usize = 64 * 1024;

        let mut total_read = 0u64;
        let mut total_written = 0u64;

        let result = (|| -> Result<()> {
            let destination = pstm
                .as_ref()
                .ok_or_else(|| windows::core::Error::from(STG_E_INVALIDPOINTER))?;

            let mut buffer = [0u8; BUFFER_SIZE];

            while total_read < cb {
                let requested = (cb - total_read).min(BUFFER_SIZE as u64) as u32;
                let mut bytes_read = 0u32;

                let hr = self.Read(buffer.as_mut_ptr().cast(), requested, &mut bytes_read);

                total_read += u64::from(bytes_read);
                hr.ok()?;

                if bytes_read == 0 {
                    break;
                }

                let mut offset = 0usize;
                let length = bytes_read as usize;

                while offset < length {
                    let mut bytes_written = 0u32;
                    let remaining = (length - offset) as u32;

                    let hr = unsafe {
                        destination.Write(
                            buffer[offset..length].as_ptr().cast(),
                            remaining,
                            Some(&mut bytes_written),
                        )
                    };

                    total_written += u64::from(bytes_written);
                    hr.ok()?;

                    if bytes_written == 0 || bytes_written > remaining {
                        return Err(STG_E_WRITEFAULT.into());
                    }

                    offset += bytes_written as usize;
                }

                if hr == S_FALSE {
                    break;
                }
            }

            Ok(())
        })();

        if !pcbread.is_null() {
            unsafe { pcbread.write(total_read) };
        }

        if !pcbwritten.is_null() {
            unsafe { pcbwritten.write(total_written) };
        }

        result
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

    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> Result<()> {
        if pstatstg.is_null() {
            return Err(STG_E_INVALIDPOINTER.into());
        }

        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return Err(STG_E_READFAULT.into()),
        };

        let size = match self.size {
            Some(size) => size,
            None => match state.reader.try_size()? {
                Some(size) => size,
                None => return Err(STG_E_INVALIDFUNCTION.into()),
            },
        };

        let stat = STATSTG {
            r#type: STGTY_STREAM.0 as u32,
            grfMode: STGM_READ,
            cbSize: size,
            ..Default::default()
        };

        unsafe {
            pstatstg.write(stat);
        }

        Ok(())
    }

    fn Clone(&self) -> Result<IStream> {
        Err(E_NOTIMPL.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor, Seek};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{ISequentialStream, STATFLAG_DEFAULT};

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
    fn seekable_reader_can_seek() {
        let mut reader = SourceReader::seekable(Cursor::new(b"hello".to_vec()));

        assert_eq!(reader.seek(SeekFrom::Start(2)).unwrap(), 2);

        let mut buffer = [0u8; 2];
        reader.read_exact(&mut buffer).unwrap();

        assert_eq!(&buffer, b"ll");
    }

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

    struct CancelOnFirstRead {
        inner: Cursor<Vec<u8>>,
        token: CancellationToken,
        read_count: Arc<AtomicUsize>,
    }

    impl Read for CancelOnFirstRead {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let len = buffer.len().min(2);
            let count = self.inner.read(&mut buffer[..len])?;

            self.read_count.fetch_add(1, Ordering::Relaxed);
            self.token.cancel();

            Ok(count)
        }
    }

    impl Seek for CancelOnFirstRead {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }

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

        let error = unsafe { source.CopyTo(&destination, 5, Some(&mut read), Some(&mut written)) }
            .unwrap_err();

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

        let error = unsafe { source.CopyTo(&destination, 5, Some(&mut read), Some(&mut written)) }
            .unwrap_err();

        assert_eq!(error.code(), E_ABORT);
        assert_eq!(read_count.load(Ordering::Relaxed), 1);

        // The reader returned 2 bytes before cancellation.
        assert_eq!(read, 2);
        assert_eq!(written, 0);
    }

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

        let error = unsafe { source.CopyTo(&destination, 5, Some(&mut read), Some(&mut written)) }
            .unwrap_err();

        assert_eq!(error.code(), STG_E_WRITEFAULT);
        assert_eq!(read, 5);
        assert_eq!(written, 0);
        assert!(bytes.lock().unwrap().is_empty());
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
}
