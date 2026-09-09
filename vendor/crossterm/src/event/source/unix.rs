#[cfg(feature = "use-dev-tty")]
pub(crate) mod tty;

#[cfg(not(feature = "use-dev-tty"))]
pub(crate) mod mio;

#[cfg(feature = "use-dev-tty")]
pub(crate) use self::tty::UnixInternalEventSource;

#[cfg(not(feature = "use-dev-tty"))]
pub(crate) use self::mio::UnixInternalEventSource;

#[cfg(all(feature = "managed-signals", feature = "use-dev-tty"))]
pub(crate) use self::tty::prepare_signals;
#[cfg(all(feature = "managed-signals", not(feature = "use-dev-tty")))]
pub(crate) use self::mio::prepare_signals;
