use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferEffect {
    None,
    Copy,
    Move,
    Link,
    Other(u32),
}

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
pub(crate) struct TransferCompletion {
    status: Mutex<TransferStatus>,
}

impl TransferCompletion {
    pub(crate) fn new() -> Self {
        Self {
            status: Mutex::new(TransferStatus::Pending),
        }
    }

    pub(crate) fn record_if_pending(
        &self,
        new_status: TransferStatus,
    ) -> std::result::Result<(), ()> {
        let mut status = self.status.lock().map_err(|_| ())?;

        if matches!(*status, TransferStatus::Pending) {
            *status = new_status;
        }

        Ok(())
    }

    pub(crate) fn status(&self) -> TransferStatus {
        let stored = self
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        *stored
    }
}

pub struct TransferHandle {
    completion: Arc<TransferCompletion>,
}

impl TransferHandle {
    pub(crate) fn new(completion: Arc<TransferCompletion>) -> Self {
        Self { completion }
    }

    pub fn status(&self) -> TransferStatus {
        self.completion.status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn initial_status_is_pending() {
        let completion = Arc::new(TransferCompletion::new());
        let handle = TransferHandle::new(completion);

        assert_eq!(handle.status(), TransferStatus::Pending);
    }

    #[test]
    fn completion_updates_handle_status() {
        let completion = Arc::new(TransferCompletion::new());
        let handle = TransferHandle::new(Arc::clone(&completion));

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
        let handle = TransferHandle::new(Arc::clone(&completion));

        let worker = std::thread::spawn(move || {
            _ = completion.record_if_pending(TransferStatus::Dropped);
        });

        worker.join().unwrap();

        assert_eq!(handle.status(), TransferStatus::Dropped);
    }
}
