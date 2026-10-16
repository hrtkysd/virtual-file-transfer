use crate::cancellation::CancellationToken;

use super::virtual_file::VirtualFile;

use std::sync::{Arc, Mutex};

/// The data-transfer operation reported by a receiving application.
///
/// An effect describes what operation was requested or reported, not whether
/// reading the file contents ultimately succeeded. Check [`TransferStatus`]
/// for the reported outcome.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferEffect {
    /// No copy, move, or link effect was reported.
    None,
    /// The receiver reported a copy operation.
    Copy,
    /// The receiver reported a move operation.
    Move,
    /// The receiver reported a link operation.
    Link,
    /// An unrecognized platform-specific effect, retaining its raw value.
    Other(u32),
}

/// A platform-specific completion status reported for a transfer.
///
/// The numeric value is intended for diagnostics. Interpret it according to
/// the originating platform, and use [`TransferStatus`] for the overall
/// transfer state.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeStatusCode {
    /// A Windows `HRESULT`, stored in its original signed 32-bit representation.
    ///
    /// For example, `0x8003001E` represents `STG_E_READFAULT`.
    WindowsHResult(i32),
    // add here for other operating system.
}

/// The observed state of a drag-and-drop operation.
///
/// A successful drop and successful delivery of file contents are different
/// events for receivers using asynchronous OLE transfers. In particular,
/// [`Self::Dropped`] is not proof that the receiver finished reading data.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    /// No terminal drag or asynchronous completion status has been recorded.
    Pending,
    /// The operation was cancelled, including cancellation observed during reading.
    Cancelled,
    /// The receiver accepted the drop operation.
    ///
    /// An asynchronous receiver may still be reading the contents. This status
    /// does not guarantee that the resulting destination file was completed.
    Dropped,
    /// The receiver reported completion of an asynchronous transfer.
    ///
    /// Inspect `succeeded`, `native_code`, and `effect` together rather than
    /// assuming that a non-`None` effect means success.
    AsyncFinished {
        /// Whether the receiver reported successful completion.
        succeeded: bool,
        /// The platform-specific completion code reported by the receiver.
        native_code: NativeStatusCode,
        /// The reported copy, move, link, or other transfer effect.
        effect: TransferEffect,
    },
    /// The reported copy, move, link, or other transfer effect.
    Failed,
}

/// A set of virtual files and a cancellation token for one transfer.
///
/// Create a request when you need access to its cancellation token before
/// starting a blocking drag operation. Functions accepting
/// `Into<TransferRequest>` can also take a `Vec<VirtualFile>` directly.
pub struct TransferRequest {
    pub(crate) files: Vec<VirtualFile>,
    pub(crate) cancellation: CancellationToken,
}

impl TransferRequest {
    /// Creates a transfer request containing the supplied virtual files.
    ///
    /// The request starts with a new, non-cancelled token. File names and
    /// transfer-level conflicts are validated when the transfer data object is
    /// created, not by this constructor.
    pub fn new(files: Vec<VirtualFile>) -> Self {
        Self {
            files,
            cancellation: CancellationToken::new(),
        }
    }
    /// Returns a clone of the request's shared cancellation token.
    ///
    /// Keep the token to request cancellation while a blocking drag operation
    /// is running, for example from another thread or UI callback.
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

/// Provides cancellation and status inspection for a drag transfer.
///
/// A handle can outlive the initial drag-and-drop call because the receiver
/// may continue transferring file contents asynchronously.
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

    /// Returns the latest recorded transfer status without waiting.
    ///
    /// For asynchronous receivers, a drop is not the same as successful file
    /// delivery. Interpret [`TransferStatus::Dropped`] accordingly.
    pub fn status(&self) -> TransferStatus {
        self.completion.status()
    }

    /// Requests cooperative cancellation of this transfer.
    ///
    /// The method does not wait for completion and cannot interrupt an already
    /// blocking source read immediately.
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
