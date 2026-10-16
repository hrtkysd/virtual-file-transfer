use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A shareable token for requesting cooperative transfer cancellation.
///
/// Clones refer to the same cancellation state. Calling [`Self::cancel`] on
/// any clone makes [`Self::is_cancelled`] return `true` on every clone.
/// Cancellation cannot be reset. An in-progress blocking read is not
/// interrupted immediately; cancellation is checked at defined points in
/// the transfer implementation.
#[derive(Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates a token in the non-cancelled state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation for every operation sharing this token.
    ///
    /// This method returns immediately and does not wait for a read or transfer
    /// to stop.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}
