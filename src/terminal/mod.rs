//! Terminal adapters. The engine owns scoring; this module owns receipt order,
//! input integrity, and reversible terminal capabilities.
mod probe;
mod reader;
mod session;

pub use probe::{ProbeReport, probe_capabilities};
pub use reader::{
    Control, Envelope, EpochClosure, EventReceiver, InputKind, Reader, ReaderOptions,
    normalize_event,
};
pub use session::{CaretStyle, Session, SessionOptions, controlling_terminal, terminal_size};
