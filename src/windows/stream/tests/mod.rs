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
