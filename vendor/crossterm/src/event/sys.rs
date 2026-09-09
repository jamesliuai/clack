#[cfg(all(unix, any(feature = "event-stream", feature = "poll-waker")))]
pub(crate) use unix::waker::Waker;
#[cfg(all(windows, any(feature = "event-stream", feature = "poll-waker")))]
pub(crate) use windows::waker::Waker;

#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;
