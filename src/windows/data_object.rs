use super::formats::{
    FileDescriptorMetadata, create_file_group_descriptor_medium, file_transfer_formats,
};

use super::stream::create_stream_medium;

use crate::virtual_file::VirtualFile;

use windows::Win32::Foundation::{
    DV_E_DVASPECT, DV_E_FORMATETC, DV_E_LINDEX, DV_E_TYMED, E_FAIL, E_INVALIDARG, E_NOTIMPL,
    OLE_E_ADVISENOTSUPPORTED, S_OK,
};
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
    IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::UI::Shell::SHCreateStdEnumFmtEtc;
use windows::core::{BOOL, Error as WindowsError, HRESULT, Ref, Result, implement};

enum DataRequest {
    FileDescriptor,
    FileContents(usize),
}

pub(crate) fn create_data_object(
    files: Vec<VirtualFile>,
) -> windows::core::Result<IDataObject> {
    let object = DataObject::new(files)?;
    Ok(object.into())
}

fn resolve_request(
    format: *const FORMATETC,
    file_count: usize,
) -> std::result::Result<DataRequest, HRESULT> {
    let Some(format) = (unsafe { format.as_ref() }) else {
        return Err(DV_E_FORMATETC);
    };

    if format.dwAspect != DVASPECT_CONTENT.0 as u32 {
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

        return Ok(DataRequest::FileDescriptor);
    }

    if format_id == formats.file_contents {
        if format.lindex < 0 {
            return Err(DV_E_LINDEX);
        }

        let index = format.lindex as usize;

        if index >= file_count {
            return Err(DV_E_LINDEX);
        }

        if format.tymed & TYMED_ISTREAM.0 as u32 == 0 {
            return Err(DV_E_TYMED);
        }

        return Ok(DataRequest::FileContents(index));
    }

    Err(DV_E_FORMATETC)
}

#[implement(IDataObject)]
pub(crate) struct DataObject {
    files: Vec<VirtualFile>,
}

impl DataObject {
    pub(crate) fn new(files: Vec<VirtualFile>) -> Result<Self> {
        if files.is_empty() {
            return Err(E_INVALIDARG.into());
        }

        Ok(Self { files })
    }
}

#[allow(non_snake_case)]
impl IDataObject_Impl for DataObject_Impl {
    fn GetData(&self, format: *const FORMATETC) -> Result<STGMEDIUM> {
        let request =
            resolve_request(format, self.files.len()).map_err(WindowsError::from_hresult)?;

        match request {
            DataRequest::FileDescriptor => {
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

            DataRequest::FileContents(index) => {
                let file = &self.files[index];

                let reader = file
                    .open()
                    .map_err(|error| WindowsError::new(E_FAIL, error.to_string()))?;

                Ok(create_stream_medium(reader, file.size()))
            }
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
                dwAspect: DVASPECT_CONTENT.0 as u32,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
            },
            FORMATETC {
                cfFormat: formats.file_contents as u16,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0 as u32,
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

    use crate::{FileSource, SourceReader};

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

        let object = DataObject::new(vec![file]).unwrap();

        let _object: IDataObject = object.into();
    }
}
