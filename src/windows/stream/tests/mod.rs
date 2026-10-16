use super::*;

mod cancellation;
mod copy_to;
mod read;
mod seek;
mod stat;

use std::io::{self, Cursor, Seek};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{ISequentialStream, STATFLAG_DEFAULT};

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

const FOUR_GIB: u64 = 1u64 << 32;

struct LargeReader {
    size: u64,
    position: u64,
}

impl LargeReader {
    fn new(size: u64) -> Self {
        Self { size, position: 0 }
    }
}

impl Read for LargeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = self
            .size
            .saturating_sub(self.position)
            .min(buf.len() as u64) as usize;

        for (index, byte) in buf[..count].iter_mut().enumerate() {
            *byte = (self.position + index as u64) as u8;
        }

        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for LargeReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = match from {
            SeekFrom::Start(pos) => i128::from(pos),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.size) + i128::from(offset),
        };

        self.position = u64::try_from(next)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid seek position"))?;

        Ok(self.position)
    }
}
