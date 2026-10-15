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
mod tests;
