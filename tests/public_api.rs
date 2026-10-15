use virtual_file_transfer::{
    CancellationToken, FactorySource, SourceReader, TransferEffect, TransferRequest,
    TransferStatus, VirtualFile,
};

use std::io::Cursor;

#[test]
fn public_transfer_api_is_usable() {
    let source = FactorySource::new(|| Ok(SourceReader::seekable(Cursor::new(b"hello".to_vec()))));

    let file = VirtualFile::new("hello.txt", Box::new(source)).unwrap();

    let request = TransferRequest::new(vec![file]);

    let token: CancellationToken = request.cancellation_token();

    assert!(!token.is_cancelled());

    token.cancel();

    assert!(token.is_cancelled());

    let effect: TransferEffect = TransferEffect::Copy;

    let status = TransferStatus::AsyncFinished {
        succeeded: true,
        native_code: 0,
        effect,
    };

    assert!(matches!(status, TransferStatus::AsyncFinished { .. }));
}
