#[cfg(any(feature = "event-stream", feature = "poll-waker"))]
pub(crate) mod waker;

#[cfg(feature = "events")]
pub(crate) mod parse;
