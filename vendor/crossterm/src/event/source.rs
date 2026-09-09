use std::{io, time::Duration};

#[cfg(any(feature = "event-stream", feature = "poll-waker"))]
use super::sys::Waker;
use super::InternalEvent;

#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

/// An interface for trying to read an `InternalEvent` within an optional `Duration`.
pub(crate) trait EventSource: Sync + Send {
    /// Tries to read an `InternalEvent` within the given duration.
    ///
    /// # Arguments
    ///
    /// * `timeout` - `None` block indefinitely until an event is available, `Some(duration)` blocks
    ///   for the given timeout
    ///
    /// Returns `Ok(None)` if there's no event available and timeout expires.
    fn try_read(&mut self, timeout: Option<Duration>) -> io::Result<Option<InternalEvent>>;

    /// Discard pending keyboard bytes and incomplete parser state at an epoch barrier.
    #[cfg(feature = "poll-waker")]
    fn discard_pending_input(&mut self) -> io::Result<()> { Ok(()) }

    /// Returns a `Waker` allowing to wake/force the `try_read` method to return `Ok(None)`.
    #[cfg(any(feature = "event-stream", feature = "poll-waker"))]
    fn waker(&self) -> Waker;
}

#[cfg(feature = "poll-waker")]
pub(crate) fn preserve_across_epoch(event: &InternalEvent) -> bool {
    if matches!(event, InternalEvent::Event(super::Event::Resize(..))) { return true; }
    #[cfg(all(unix, feature = "managed-signals"))]
    if matches!(event, InternalEvent::Event(super::Event::Signal(_))) { return true; }
    false
}
