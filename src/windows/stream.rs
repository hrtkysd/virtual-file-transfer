use crate::source_reader::{SourceReader, SourceReaderError};
use std::cell::RefCell;
use std::ffi::c_void;
use std::io::{Read, SeekFrom};
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{
    E_NOTIMPL, S_FALSE, S_OK, STG_E_ACCESSDENIED, STG_E_INVALIDFUNCTION, STG_E_INVALIDPOINTER,
    STG_E_READFAULT, STG_E_SEEKERROR,
};
use windows::Win32::System::Com::{
    ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE, STATFLAG, STATSTG, STGC, STGM_READ,
    STGMEDIUM, STGMEDIUM_0, STGTY_STREAM, STREAM_SEEK, STREAM_SEEK_CUR, STREAM_SEEK_END,
    STREAM_SEEK_SET, TYMED_ISTREAM,
};
use windows::core::{HRESULT, Ref, Result, implement};

#[implement(IStream)]
pub(crate) struct Stream {
    reader: RefCell<SourceReader>,
    size: Option<u64>,
}

impl Stream {
    pub(crate) fn new(reader: SourceReader, size: Option<u64>) -> Self {
        Self {
            reader: RefCell::new(reader),
            size,
        }
    }
}

pub(crate) fn create_stream_medium(reader: SourceReader, size: Option<u64>) -> STGMEDIUM {
    let stream: IStream = Stream::new(reader, size).into();

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
            unsafe {
                *pcbread = 0;
            }
        }

        if cb == 0 {
            return S_OK;
        }

        if pv.is_null() {
            return STG_E_INVALIDPOINTER;
        }

        let buffer = unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>(), cb as usize) };

        match self.reader.borrow_mut().read(buffer) {
            Ok(read) => {
                if !pcbread.is_null() {
                    unsafe {
                        *pcbread = read as u32;
                    }
                }

                if read == cb as usize { S_OK } else { S_FALSE }
            }

            Err(_) => STG_E_READFAULT,
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

        match self.reader.borrow_mut().seek(seek_from) {
            Ok(position) => {
                if !plibnewposition.is_null() {
                    unsafe {
                        plibnewposition.write(position);
                    }
                }

                Ok(())
            }

            Err(SourceReaderError::NotSeekable) => Err(STG_E_INVALIDFUNCTION.into()),

            Err(SourceReaderError::Io(_)) => Err(STG_E_SEEKERROR.into()),
        }
    }

    fn SetSize(&self, _libnewsize: u64) -> Result<()> {
        Err(STG_E_ACCESSDENIED.into())
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

    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> Result<()> {
        if pstatstg.is_null() {
            return Err(STG_E_INVALIDPOINTER.into());
        }

        let mut stat = STATSTG::default();

        stat.r#type = STGTY_STREAM.0 as u32;
        stat.grfMode = STGM_READ;
        stat.cbSize = match self.size {
            Some(size) => size,
            None => {
                let mut reader = self.reader.borrow_mut();

                match reader.try_size()? {
                    Some(size) => size,
                    None => {
                        return Err(STG_E_INVALIDFUNCTION.into());
                    }
                }
            }
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
    use std::io::Cursor;
    use windows::Win32::System::Com::{ISequentialStream, STATFLAG_DEFAULT};

    #[test]
    fn reads_from_com_stream() {
        let reader = SourceReader::seekable(std::io::Cursor::new(b"hello".to_vec()));
        let stream: IStream = Stream::new(reader, Some(5)).into();
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

        let stream: IStream = Stream::new(reader, Some(5)).into();
        let mut stat = STATSTG::default();

        unsafe {
            stream.Stat(&mut stat, STATFLAG_DEFAULT).unwrap();
        }

        assert_eq!(stat.cbSize, 5);
    }

    #[test]
    fn stat_fails_for_unknown_size_streaming_reader() {
        let reader = SourceReader::streaming(std::io::Cursor::new(b"hello".to_vec()));

        let stream: IStream = Stream::new(reader, None).into();
        let mut stat = STATSTG::default();

        let error = unsafe { stream.Stat(&mut stat, STATFLAG_DEFAULT) }.unwrap_err();

        assert_eq!(error.code(), STG_E_INVALIDFUNCTION);
    }

    #[test]
    fn stat_derives_size_for_unknown_seekable_reader() {
        let reader = SourceReader::seekable(Cursor::new(b"hello".to_vec()));

        let stream: IStream = Stream::new(reader, None).into();
        let mut stat = STATSTG::default();

        unsafe {
            stream.Stat(&mut stat, STATFLAG_DEFAULT).unwrap();
        }

        assert_eq!(stat.cbSize, 5);
    }

    #[test]
    fn com_stream_can_seek() {
        let reader = SourceReader::seekable(Cursor::new(b"hello".to_vec()));

        let stream: IStream = Stream::new(reader, None).into();

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

        let stream: IStream = Stream::new(reader, Some(5)).into();

        let error = unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.unwrap_err();

        assert_eq!(error.code(), STG_E_INVALIDFUNCTION);
    }
}
