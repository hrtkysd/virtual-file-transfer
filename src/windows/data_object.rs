mod async_operation;

use super::formats::{
    FileDescriptorMetadata, create_file_group_descriptor_medium, file_transfer_formats,
};
use super::stream::create_stream_medium;
use super::validation::validate_files;

use self::async_operation::AsyncState;

use crate::cancellation::CancellationToken;
use crate::transfer::{TransferCompletion, TransferRequest};
use crate::virtual_file::VirtualFile;

use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{
    DV_E_DVASPECT, DV_E_FORMATETC, DV_E_LINDEX, DV_E_TYMED, E_FAIL, E_NOTIMPL,
    OLE_E_ADVISENOTSUPPORTED, S_OK,
};
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
    IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::UI::Shell::{IDataObjectAsyncCapability, SHCreateStdEnumFmtEtc};
use windows::core::{BOOL, Error as WindowsError, HRESULT, Ref, Result, implement};

enum DataRequest {
    Descriptor,
    Contents(usize),
    ContentsUnspecified,
}

pub(crate) fn create_data_object(request: TransferRequest) -> crate::Result<IDataObject> {
    let TransferRequest {
        files,
        cancellation,
    } = request;

    let data_object: IDataObject = DataObject::new(files, cancellation, None)?.into();

    Ok(data_object)
}

fn resolve_request(
    format: *const FORMATETC,
    file_count: usize,
) -> std::result::Result<DataRequest, HRESULT> {
    let Some(format) = (unsafe { format.as_ref() }) else {
        return Err(DV_E_FORMATETC);
    };

    if format.dwAspect != DVASPECT_CONTENT.0 {
        return Err(DV_E_DVASPECT);
    }

    let formats = file_transfer_formats().map_err(|error| error.code())?;

    let format_id = format.cfFormat as u32;

    if format_id == formats.file_descriptor {
        if format.lindex != -1 {
            return Err(DV_E_LINDEX);
        }

        if format.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
            return Err(DV_E_TYMED);
        }

        return Ok(DataRequest::Descriptor);
    }

    if format_id == formats.file_contents {
        if format.tymed & TYMED_ISTREAM.0 as u32 == 0 {
            return Err(DV_E_TYMED);
        }

        let index = match format.lindex {
            -1 => return Ok(DataRequest::ContentsUnspecified),
            index if index >= 0 => index as usize,
            _ => return Err(DV_E_LINDEX),
        };

        if index >= file_count {
            return Err(DV_E_LINDEX);
        }

        return Ok(DataRequest::Contents(index));
    }

    Err(DV_E_FORMATETC)
}

#[implement(IDataObject, IDataObjectAsyncCapability)]
pub(crate) struct DataObject {
    files: Vec<VirtualFile>,
    async_state: Mutex<AsyncState>,
    token: CancellationToken,
    completion: Option<Arc<TransferCompletion>>,
}

impl DataObject {
    pub(crate) fn new(
        files: Vec<VirtualFile>,
        token: CancellationToken,
        completion: Option<Arc<TransferCompletion>>,
    ) -> crate::Result<Self> {
        validate_files(&files)?;

        Ok(Self {
            files,
            async_state: Mutex::new(AsyncState::default()),
            token,
            completion,
        })
    }

    pub(crate) fn finish_without_async(&self) -> Result<()> {
        let released = {
            // Recover the poisoned mutex to release the COM self-reference
            // even if a previous operation panicked.
            let mut state = self
                .async_state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            if state.in_operation {
                return Ok(());
            }

            state.enabled = false;
            state.self_ref.take()
        };

        drop(released);
        Ok(())
    }
}

#[allow(non_snake_case)]
impl IDataObject_Impl for DataObject_Impl {
    fn GetData(&self, format: *const FORMATETC) -> Result<STGMEDIUM> {
        let request =
            resolve_request(format, self.files.len()).map_err(WindowsError::from_hresult)?;

        match request {
            DataRequest::Descriptor => {
                let descriptors = self
                    .files
                    .iter()
                    .map(|file| FileDescriptorMetadata {
                        name: file.name(),
                        size: file.size(),
                    })
                    .collect::<Vec<_>>();

                create_file_group_descriptor_medium(&descriptors)
            }

            DataRequest::Contents(index) => {
                let file = &self.files[index];

                let reader = file
                    .open()
                    .map_err(|error| WindowsError::new(E_FAIL, error.to_string()))?;

                Ok(create_stream_medium(
                    reader,
                    file.size(),
                    self.token.clone(),
                    self.completion.clone(),
                ))
            }
            DataRequest::ContentsUnspecified => Err(DV_E_LINDEX.into()),
        }
    }

    fn GetDataHere(&self, _format: *const FORMATETC, _medium: *mut STGMEDIUM) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
        match resolve_request(format, self.files.len()) {
            Ok(_) => S_OK,
            Err(error) => error,
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _format_in: *const FORMATETC,
        _format_out: *mut FORMATETC,
    ) -> HRESULT {
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _format: *const FORMATETC,
        _medium: *const STGMEDIUM,
        _release: BOOL,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, direction: u32) -> Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }

        let formats = file_transfer_formats()?;

        let formats = [
            FORMATETC {
                cfFormat: formats.file_descriptor as u16,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
            },
            FORMATETC {
                cfFormat: formats.file_contents as u16,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0,
                lindex: 0,
                tymed: TYMED_ISTREAM.0 as u32,
            },
        ];

        unsafe { SHCreateStdEnumFmtEtc(&formats) }
    }

    fn DAdvise(
        &self,
        _format: *const FORMATETC,
        _advf: u32,
        _sink: Ref<'_, IAdviseSink>,
    ) -> Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _connection: u32) -> Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    use crate::cancellation::CancellationToken;
    use crate::{Error, FileSource, InputError, SourceReader, TransferRequest};

    use windows::Win32::Foundation::{DV_E_LINDEX, E_ABORT};
    use windows::Win32::System::Com::{
        DVASPECT_CONTENT, FORMATETC, IDataObject, ISequentialStream, TYMED_HGLOBAL, TYMED_ISTREAM,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::ReleaseStgMedium;
    use windows::Win32::UI::Shell::{FD_FILESIZE, FD_UNICODE, FILEDESCRIPTORW};

    struct StubSource;

    impl FileSource for StubSource {
        fn open(&self) -> io::Result<SourceReader> {
            Ok(SourceReader::seekable(std::io::Cursor::new(
                b"hello".to_vec(),
            )))
        }
    }

    #[test]
    fn create_data_object() {
        let file = VirtualFile::new("hello.txt", Box::new(StubSource)).unwrap();

        let object = DataObject::new(vec![file], CancellationToken::new(), None).unwrap();

        let _object: IDataObject = object.into();
    }

    #[test]
    fn cancellation_propagates_to_content_stream() {
        let token = CancellationToken::new();

        let file = VirtualFile::new("hello.txt", Box::new(StubSource)).unwrap();

        let data_object: IDataObject = DataObject::new(vec![file], token.clone(), None)
            .unwrap()
            .into();

        let format = FORMATETC {
            cfFormat: file_transfer_formats().unwrap().file_contents as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: 0,
            tymed: TYMED_ISTREAM.0 as u32,
        };

        let mut medium = unsafe { data_object.GetData(&format) }.unwrap();

        assert_eq!(medium.tymed, TYMED_ISTREAM.0 as u32);

        let stream = unsafe { (*medium.u.pstm).as_ref().unwrap().clone() };

        unsafe {
            ReleaseStgMedium(&mut medium);
        }

        let sequential: &ISequentialStream = (&stream).into();

        token.cancel();

        let mut buffer = [0u8; 5];
        let mut bytes_read = u32::MAX;

        let hr = unsafe {
            sequential.Read(
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                Some(&mut bytes_read),
            )
        };

        assert_eq!(hr, E_ABORT);
        assert_eq!(bytes_read, 0);
    }

    struct MemorySource(&'static [u8]);

    impl FileSource for MemorySource {
        fn open(&self) -> io::Result<SourceReader> {
            Ok(SourceReader::seekable(io::Cursor::new(self.0)))
        }
    }

    #[test]
    fn get_data_returns_multiple_file_descriptors() {
        const LARGE_SIZE: u64 = (1u64 << 32) + 123;

        let files = vec![
            VirtualFile::new("hello.txt", Box::new(MemorySource(b"hello")))
                .unwrap()
                .with_size(5),
            VirtualFile::new("large.bin", Box::new(MemorySource(b"unused")))
                .unwrap()
                .with_size(LARGE_SIZE),
        ];

        let data_object: IDataObject = DataObject::new(files, CancellationToken::new(), None)
            .unwrap()
            .into();

        let format = FORMATETC {
            cfFormat: file_transfer_formats().unwrap().file_descriptor as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };

        let mut medium = unsafe { data_object.GetData(&format) }.unwrap();

        assert_eq!(medium.tymed, TYMED_HGLOBAL.0 as u32);

        let hglobal = unsafe { medium.u.hGlobal };

        let required_size = std::mem::size_of::<u32>() + 2 * std::mem::size_of::<FILEDESCRIPTORW>();

        assert!(unsafe { GlobalSize(hglobal) } >= required_size);

        let ptr = unsafe { GlobalLock(hglobal) }.cast::<u8>();
        assert!(!ptr.is_null());

        // FILEGROUPDESCRIPTORW:
        //   UINT cItems;
        //   FILEDESCRIPTORW fgd[cItems];
        //
        // The descriptor array begins at offset 4, which may
        // not satisfy FILEDESCRIPTORW's alignment requirements.
        let (count, descriptors) = unsafe {
            let count = ptr.cast::<u32>().read_unaligned();

            let descriptors = (0..2)
                .map(|index| {
                    ptr.add(
                        std::mem::size_of::<u32>() + index * std::mem::size_of::<FILEDESCRIPTORW>(),
                    )
                    .cast::<FILEDESCRIPTORW>()
                    .read_unaligned()
                })
                .collect::<Vec<_>>();

            _ = GlobalUnlock(hglobal);
            ReleaseStgMedium(&mut medium);

            (count, descriptors)
        };

        assert_eq!(count, 2);

        for (descriptor, (expected_name, expected_size)) in descriptors
            .iter()
            .zip([("hello.txt", 5u64), ("large.bin", LARGE_SIZE)])
        {
            let filename = descriptor.cFileName;
            let flags = descriptor.dwFlags;
            let size_high = descriptor.nFileSizeHigh;
            let size_low = descriptor.nFileSizeLow;

            let name_end = filename.iter().position(|&c| c == 0).unwrap();

            let name = String::from_utf16(&filename[..name_end]).unwrap();

            let size = (u64::from(size_high) << 32) | u64::from(size_low);

            assert_eq!(name, expected_name);
            assert_eq!(size, expected_size);

            assert_ne!(flags & FD_FILESIZE.0 as u32, 0);
            assert_ne!(flags & FD_UNICODE.0 as u32, 0);
        }
    }

    #[test]
    fn get_data_returns_contents_by_lindex() {
        let files = vec![
            VirtualFile::new("first.txt", Box::new(MemorySource(b"first")))
                .unwrap()
                .with_size(5),
            VirtualFile::new("second.txt", Box::new(MemorySource(b"second")))
                .unwrap()
                .with_size(6),
        ];

        let data_object: IDataObject = DataObject::new(files, CancellationToken::new(), None)
            .unwrap()
            .into();

        let mut format = FORMATETC {
            cfFormat: file_transfer_formats().unwrap().file_contents as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: 0,
            tymed: TYMED_ISTREAM.0 as u32,
        };

        for (index, expected) in [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .enumerate()
        {
            format.lindex = index as i32;

            let mut medium = unsafe { data_object.GetData(&format) }.unwrap();

            assert_eq!(medium.tymed, TYMED_ISTREAM.0 as u32);

            // Clone the COM reference before releasing STGMEDIUM.
            let stream = unsafe { (*medium.u.pstm).as_ref().unwrap().clone() };

            unsafe {
                ReleaseStgMedium(&mut medium);
            }

            let sequential: &ISequentialStream = (&stream).into();

            let mut buffer = vec![0u8; expected.len()];
            let mut bytes_read = 0u32;

            let hr = unsafe {
                sequential.Read(
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    Some(&mut bytes_read),
                )
            };

            assert_eq!(hr, S_OK);
            assert_eq!(bytes_read as usize, expected.len());
            assert_eq!(buffer, expected);
        }

        // Index outside the available file range.
        format.lindex = 2;

        let error = unsafe { data_object.GetData(&format) }
            .err()
            .expect("GetData should reject an invalid lindex");

        assert_eq!(error.code(), DV_E_LINDEX);
    }

    fn file(name: &str) -> VirtualFile {
        VirtualFile::new(name, Box::new(StubSource)).unwrap()
    }

    #[test]
    fn accepts_valid_file_paths() {
        let files = vec![
            file("folder/a.txt"),
            file("folder/sub/b.txt"),
            file("other/c.txt"),
        ];

        assert!(DataObject::new(files, CancellationToken::new(), None).is_ok());
    }

    #[test]
    fn rejects_duplicate_file_paths() {
        let files = vec![file("folder/a.txt"), file(r"folder\a.txt")];

        let result = DataObject::new(files, CancellationToken::new(), None);

        assert!(matches!(
            result,
            Err(Error::InvalidInput(InputError::DuplicatePath))
        ));
    }

    #[test]
    fn rejects_file_directory_conflict() {
        let files = vec![file("folder"), file("folder/sub/a.txt")];

        let result = DataObject::new(files, CancellationToken::new(), None);

        assert!(matches!(
            result,
            Err(Error::InvalidInput(InputError::ConflictPath))
        ));
    }

    #[test]
    fn create_data_object_propagates_validation_error() {
        let request = TransferRequest::new(vec![file("a.txt"), file("A.txt")]);

        let result = super::create_data_object(request);

        assert!(matches!(
            result,
            Err(Error::InvalidInput(InputError::DuplicatePath))
        ));
    }

    #[test]
    fn rejects_empty_transfer() {
        let result = DataObject::new(vec![], CancellationToken::new(), None);

        assert!(matches!(
            result,
            Err(Error::InvalidInput(InputError::EmptyTransfer))
        ));
    }
}
