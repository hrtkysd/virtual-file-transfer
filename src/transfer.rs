use crate::cancellation::CancellationToken;

use super::virtual_file::VirtualFile;

use std::sync::{Arc, Mutex};

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferEffect {
    None,
    Copy,
    Move,
    Link,
    Other(u32),
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Cancelled,
    Dropped,
    AsyncFinished {
        succeeded: bool,
        native_code: i32,
        effect: TransferEffect,
    },
    Failed,
}

pub struct TransferRequest {
    pub(crate) files: Vec<VirtualFile>,
    pub(crate) cancellation: CancellationToken,
}

impl TransferRequest {
    pub fn new(files: Vec<VirtualFile>) -> Self {
        Self {
            files,
            cancellation: CancellationToken::new(),
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

impl From<Vec<VirtualFile>> for TransferRequest {
    fn from(files: Vec<VirtualFile>) -> Self {
        Self::new(files)
    }
}

#[derive(Clone, Copy, Debug)]
enum CompletionState {
    Pending,
    ReadAborted,
    Finished(TransferStatus),
}

pub(crate) struct TransferCompletion {
    state: Mutex<CompletionState>,
}

impl TransferCompletion {
    pub(crate) fn new() -> Self {
        TransferCompletion {
            state: Mutex::new(CompletionState::Pending),
        }
    }

    pub(crate) fn mark_as_read_aborted(&self) -> std::result::Result<(), ()> {
        let mut state = self.state.lock().map_err(|_| ())?;
        if matches!(*state, CompletionState::Pending) {
            *state = CompletionState::ReadAborted;
        }
        Ok(())
    }

    pub(crate) fn record_if_pending(
        &self,
        new_status: TransferStatus,
    ) -> std::result::Result<(), ()> {
        let mut state = self.state.lock().map_err(|_| ())?;
        *state = match *state {
            CompletionState::Pending => CompletionState::Finished(new_status),

            CompletionState::ReadAborted => CompletionState::Finished(TransferStatus::Cancelled),

            CompletionState::Finished(_) => return Ok(()),
        };
        Ok(())
    }

    pub(crate) fn status(&self) -> TransferStatus {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());

        match *state {
            CompletionState::Pending | CompletionState::ReadAborted => TransferStatus::Pending,
            CompletionState::Finished(status) => status,
        }
    }
}

pub struct TransferHandle {
    completion: Arc<TransferCompletion>,
    cancellation: CancellationToken,
}

impl TransferHandle {
    pub(crate) fn new(completion: Arc<TransferCompletion>, token: CancellationToken) -> Self {
        Self {
            completion,
            cancellation: token,
        }
    }

    pub fn status(&self) -> TransferStatus {
        self.completion.status()
    }

    pub fn cancel(&self) {
        self.cancellation.cancel()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn initial_status_is_pending() {
        let completion = Arc::new(TransferCompletion::new());
        let handle = TransferHandle::new(completion, CancellationToken::new());

        assert_eq!(handle.status(), TransferStatus::Pending);
    }

    #[test]
    fn completion_updates_handle_status() {
        let completion = Arc::new(TransferCompletion::new());
        let handle = TransferHandle::new(Arc::clone(&completion), CancellationToken::new());

        _ = completion.record_if_pending(TransferStatus::Dropped);

        assert_eq!(handle.status(), TransferStatus::Dropped);
    }

    #[test]
    fn completed_status_cannot_be_overwritten() {
        let completion = TransferCompletion::new();

        _ = completion.record_if_pending(TransferStatus::Cancelled);
        _ = completion.record_if_pending(TransferStatus::Dropped);

        assert_eq!(completion.status(), TransferStatus::Cancelled);
    }

    #[test]
    fn completion_is_shared_across_threads() {
        let completion = Arc::new(TransferCompletion::new());
        let handle = TransferHandle::new(Arc::clone(&completion), CancellationToken::new());

        let worker = std::thread::spawn(move || {
            _ = completion.record_if_pending(TransferStatus::Dropped);
        });

        worker.join().unwrap();

        assert_eq!(handle.status(), TransferStatus::Dropped);
    }

    #[test]
    fn request_starts_not_cancelled() {
        let request = TransferRequest::new(vec![]);

        assert!(!request.cancellation_token().is_cancelled());
    }

    #[test]
    fn request_preserves_cancellation_after_move() {
        let request = TransferRequest::new(vec![]);
        let token = request.cancellation_token();

        let TransferRequest {
            files,
            cancellation,
        } = request;

        assert!(files.is_empty());
        assert!(!cancellation.is_cancelled());

        token.cancel();

        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn request_can_be_created_from_files() {
        let files: Vec<VirtualFile> = vec![];

        let request: TransferRequest = files.into();

        assert!(request.files.is_empty());
        assert!(!request.cancellation_token().is_cancelled());
    }
}
