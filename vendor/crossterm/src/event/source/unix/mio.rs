use std::{collections::VecDeque, io, time::Duration};

use mio::{unix::SourceFd, Events, Interest, Poll, Token};
use signal_hook_mio::v1_0::Signals;

#[cfg(any(feature = "event-stream", feature = "poll-waker"))]
use crate::event::sys::Waker;
use crate::event::{
    source::EventSource, sys::unix::parse::parse_event, timeout::PollTimeout, Event, InternalEvent,
};
use crate::terminal::sys::file_descriptor::{tty_fd, FileDesc};

// Tokens to identify file descriptor
const TTY_TOKEN: Token = Token(0);
const SIGNAL_TOKEN: Token = Token(1);
#[cfg(any(feature = "event-stream", feature = "poll-waker"))]
const WAKE_TOKEN: Token = Token(2);

// I (@zrzka) wasn't able to read more than 1_022 bytes when testing
// reading on macOS/Linux -> we don't need bigger buffer and 1k of bytes
// is enough.
const TTY_BUFFER_SIZE: usize = 1_024;

// Install the signal-only self-pipe before terminal modes change; the sole
// input owner takes it later. This preparation does not open/read the keyboard.
#[cfg(feature = "managed-signals")]
static PREPARED_SIGNALS: parking_lot::Mutex<(bool, Option<Signals>)> = parking_lot::const_mutex((false, None));

#[cfg(feature = "managed-signals")]
fn new_managed_signals() -> io::Result<Signals> {
    Signals::new([
        signal_hook::consts::SIGWINCH, signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM, signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGTSTP, signal_hook::consts::SIGCONT,
    ])
}

#[cfg(feature = "managed-signals")]
pub(crate) fn prepare_signals() -> io::Result<()> {
    let mut prepared = PREPARED_SIGNALS.lock();
    if !prepared.0 && prepared.1.is_none() { prepared.1 = Some(new_managed_signals()?); }
    Ok(())
}

#[cfg(feature = "managed-signals")]
fn take_managed_signals() -> io::Result<Signals> {
    let mut prepared = PREPARED_SIGNALS.lock();
    let signals = match prepared.1.take() {
        Some(signals) => signals,
        None => new_managed_signals()?,
    };
    prepared.0 = true;
    Ok(signals)
}

pub(crate) struct UnixInternalEventSource {
    poll: Poll,
    events: Events,
    parser: Parser,
    tty_buffer: [u8; TTY_BUFFER_SIZE],
    tty_fd: FileDesc<'static>,
    signals: Signals,
    #[cfg(any(feature = "event-stream", feature = "poll-waker"))]
    waker: Waker,
}

impl UnixInternalEventSource {
    pub fn new() -> io::Result<Self> {
        UnixInternalEventSource::from_file_descriptor(tty_fd()?)
    }

    pub(crate) fn from_file_descriptor(input_fd: FileDesc<'static>) -> io::Result<Self> {
        let poll = Poll::new()?;
        let registry = poll.registry();

        let tty_raw_fd = input_fd.raw_fd();
        let mut tty_ev = SourceFd(&tty_raw_fd);
        registry.register(&mut tty_ev, TTY_TOKEN, Interest::READABLE)?;

        #[cfg(not(feature = "managed-signals"))]
        let mut signals = Signals::new([signal_hook::consts::SIGWINCH])?;
        #[cfg(feature = "managed-signals")]
        let mut signals = take_managed_signals()?;
        registry.register(&mut signals, SIGNAL_TOKEN, Interest::READABLE)?;

        #[cfg(any(feature = "event-stream", feature = "poll-waker"))]
        let waker = Waker::new(registry, WAKE_TOKEN)?;

        Ok(UnixInternalEventSource {
            poll,
            events: Events::with_capacity(3),
            parser: Parser::default(),
            tty_buffer: [0u8; TTY_BUFFER_SIZE],
            tty_fd: input_fd,
            signals,
            #[cfg(any(feature = "event-stream", feature = "poll-waker"))]
            waker,
        })
    }
}

impl EventSource for UnixInternalEventSource {
    fn try_read(&mut self, timeout: Option<Duration>) -> io::Result<Option<InternalEvent>> {
        if let Some(event) = self.parser.next() {
            return event.map(Some);
        }

        let timeout = PollTimeout::new(timeout);
        let service_started = std::time::Instant::now();

        loop {
            if let Err(e) = self.poll.poll(&mut self.events, timeout.leftover()) {
                // Mio will throw an interrupted error in case of cursor position retrieval. We need to retry until it succeeds.
                // Previous versions of Mio (< 0.7) would automatically retry the poll call if it was interrupted (if EINTR was returned).
                // https://docs.rs/mio/0.7.0/mio/struct.Poll.html#notes
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                } else {
                    return Err(e);
                }
            };

            if self.events.is_empty() {
                // No readiness events = timeout
                return Ok(None);
            }

            for token in self.events.iter().map(|x| x.token()) {
                match token {
                    TTY_TOKEN => {
                        loop {
                            match self.tty_fd.read(&mut self.tty_buffer) {
                                Ok(read_count) => {
                                    if read_count == 0 {
                                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "terminal input closed"));
                                    }
                                    if read_count > 0 {
                                        self.parser.advance(
                                            &self.tty_buffer[..read_count],
                                            read_count == TTY_BUFFER_SIZE,
                                        );
                                    }
                                }
                                Err(e) => {
                                    // No more data to read at the moment. We will receive another event
                                    if e.kind() == io::ErrorKind::WouldBlock {
                                        break;
                                    }
                                    // once more data is available to read.
                                    else if e.kind() == io::ErrorKind::Interrupted {
                                        continue;
                                    }
                                }
                            };

                            if let Some(event) = self.parser.next() {
                                return event.map(Some);
                            }
                            if timeout.elapsed() { return Ok(None); }
                            if service_started.elapsed() >= Duration::from_millis(1) {
                                return Err(io::Error::new(io::ErrorKind::Interrupted, "yield incomplete terminal input"));
                            }
                        }
                    }
                    SIGNAL_TOKEN => {
                        for signal in self.signals.pending() {
                            if signal == signal_hook::consts::SIGWINCH {
                                let (width, height) = crate::terminal::size()?;
                                self.parser.internal_events.push_back(Ok(InternalEvent::Event(Event::Resize(width, height))));
                            } else {
                                #[cfg(feature = "managed-signals")]
                                self.parser.internal_events.push_back(Ok(InternalEvent::Event(Event::Signal(signal))));
                            }
                        }
                        if let Some(event) = self.parser.next() { return event.map(Some); }
                    }
                    #[cfg(any(feature = "event-stream", feature = "poll-waker"))]
                    WAKE_TOKEN => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "Poll operation was woken up by `Waker::wake`",
                        ));
                    }
                    _ => unreachable!("Synchronize Evented handle registration & token handling"),
                }
            }

            // Processing above can take some time, check if timeout expired
            if timeout.elapsed() {
                return Ok(None);
            }
        }
    }

    #[cfg(feature = "poll-waker")]
    fn discard_pending_input(&mut self) -> io::Result<()> {
        // A fresh controlling-terminal handle also supports the optional libc
        // descriptor backend without introducing a new unsafe conversion.
        let terminal = std::fs::File::open("/dev/tty")?;
        rustix::termios::tcflush(&terminal, rustix::termios::QueueSelector::IFlush)?;
        self.parser.buffer.clear();
        #[cfg(feature = "associated-text")]
        { self.parser.discard = None; }
        self.parser.internal_events.retain(|event| event.as_ref().is_ok_and(
            super::super::preserve_across_epoch));
        Ok(())
    }

    #[cfg(any(feature = "event-stream", feature = "poll-waker"))]
    fn waker(&self) -> Waker {
        self.waker.clone()
    }
}

//
// Following `Parser` structure exists for two reasons:
//
//  * mimic anes Parser interface
//  * move the advancing, parsing, ... stuff out of the `try_read` method
//
#[derive(Debug)]
struct Parser {
    buffer: Vec<u8>,
    #[cfg(feature = "associated-text")]
    discard: Option<Discard>,
    internal_events: VecDeque<io::Result<InternalEvent>>,
}

#[cfg(feature = "associated-text")]
#[derive(Debug)]
enum Discard { Escape, Paste(usize) }

impl Default for Parser {
    fn default() -> Self {
        Parser {
            // This buffer is used for -> 1 <- ANSI escape sequence. Are we
            // aware of any ANSI escape sequence that is bigger? Can we make
            // it smaller?
            //
            // Probably not worth spending more time on this as "there's a plan"
            // to use the anes crate parser.
            buffer: Vec::with_capacity(256),
            #[cfg(feature = "associated-text")]
            discard: None,
            // TTY_BUFFER_SIZE is 1_024 bytes. How many ANSI escape sequences can
            // fit? What is an average sequence length? Let's guess here
            // and say that the average ANSI escape sequence length is 8 bytes. Thus
            // the buffer size should be 1024/8=128 to avoid additional allocations
            // when processing large amounts of data.
            //
            // There's no need to make it bigger, because when you look at the `try_read`
            // method implementation, all events are consumed before the next TTY_BUFFER
            // is processed -> events pushed.
            internal_events: VecDeque::with_capacity(128),
        }
    }
}

impl Parser {
    fn advance(&mut self, buffer: &[u8], more: bool) {
        for (idx, byte) in buffer.iter().enumerate() {
            let more = idx + 1 < buffer.len() || more;

            #[cfg(feature = "associated-text")]
            if let Some(discard) = self.discard.as_mut() {
                let ended = match discard {
                    Discard::Escape => (0x40..=0x7e).contains(byte),
                    Discard::Paste(matched) => {
                        const END: &[u8] = b"\x1b[201~";
                        *matched = if *byte == END[*matched] { *matched + 1 }
                            else { usize::from(*byte == END[0]) };
                        *matched == END.len()
                    }
                };
                if ended {
                    #[cfg(feature = "bracketed-paste")]
                    if matches!(discard, Discard::Paste(_)) {
                        self.internal_events.push_back(Ok(InternalEvent::Event(Event::Paste(String::new()))));
                    }
                    self.discard = None;
                }
                continue;
            }
            self.buffer.push(*byte);
            #[cfg(feature = "associated-text")]
            if self.buffer.len() > 128 * 1024 {
                if self.buffer.starts_with(b"\x1b[200~") {
                    const END: &[u8] = b"\x1b[201~";
                    if self.buffer.ends_with(END) {
                        #[cfg(feature = "bracketed-paste")]
                        self.internal_events.push_back(Ok(InternalEvent::Event(Event::Paste(String::new()))));
                    } else {
                        let matched = (1..END.len()).rev().find(|length| self.buffer.ends_with(&END[..*length])).unwrap_or(0);
                        self.discard = Some(Discard::Paste(matched));
                    }
                } else {
                    self.internal_events.push_back(Err(io::Error::new(io::ErrorKind::InvalidData,
                        "terminal escape sequence exceeds 128 KiB")));
                    if !(0x40..=0x7e).contains(byte) { self.discard = Some(Discard::Escape); }
                }
                self.buffer.clear();
                continue;
            }

            match parse_event(&self.buffer, more) {
                Ok(Some(ie)) => {
                    self.internal_events.push_back(Ok(ie));
                    self.buffer.clear();
                }
                Ok(None) => {
                    // Event can't be parsed, because we don't have enough bytes for
                    // the current sequence. Keep the buffer and process next bytes.
                }
                Err(error) => {
                    #[cfg(feature = "associated-text")]
                    self.internal_events.push_back(Err(error));
                    #[cfg(not(feature = "associated-text"))]
                    let _ = error;
                    // Event can't be parsed (not enough parameters, parameter is not a number, ...).
                    // Clear the buffer and continue with another sequence.
                    self.buffer.clear();
                }
            }
        }
    }
}

impl Iterator for Parser {
    type Item = io::Result<InternalEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        self.internal_events.pop_front()
    }
}


#[cfg(all(test, feature = "associated-text"))]
mod ordered_error_tests {
    use super::*;

    #[test]
    fn malformed_associated_text_is_an_ordered_error_not_silent_loss() {
        let mut parser = Parser::default();
        parser.advance(b"a\x1b[97;1;55296ub", false);
        assert!(matches!(parser.next(), Some(Ok(InternalEvent::Event(Event::Key(_))))));
        assert!(matches!(parser.next(), Some(Err(_))));
        assert!(matches!(parser.next(), Some(Ok(InternalEvent::Event(Event::Key(_))))));
        assert!(parser.next().is_none());
    }
}

#[cfg(all(test, feature = "poll-waker", not(feature = "libc")))]
mod poll_waker_tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn closed_terminal_input_reports_eof_instead_of_spinning() {
        let (input, writer) = UnixStream::pair().unwrap();
        input.set_nonblocking(true).unwrap();
        let mut source = UnixInternalEventSource::from_file_descriptor(FileDesc::Owned(input.into())).unwrap();
        drop(writer);
        let error = source.try_read(Some(Duration::from_secs(1))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn poll_can_be_woken_from_another_thread_without_keyboard_input() {
        let (input, _writer) = UnixStream::pair().unwrap();
        input.set_nonblocking(true).unwrap();
        let mut source = UnixInternalEventSource::from_file_descriptor(FileDesc::Owned(input.into())).unwrap();
        let waker = source.waker();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            waker.wake().unwrap();
        });
        let started = std::time::Instant::now();
        let error = source.try_read(Some(Duration::from_secs(10))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(2));
        sender.join().unwrap();
    }
}

#[cfg(all(test, feature = "associated-text", feature = "bracketed-paste"))]
mod bounded_parser_tests {
    use super::*;

    #[test]
    fn huge_paste_is_one_atomic_event_and_memory_stays_bounded() {
        let mut parser = Parser::default();
        parser.advance(b"\x1b[200~", true);
        for _ in 0..1024 { parser.advance(&[b'a'; 1024], true); }
        assert!(parser.buffer.capacity() <= 256 * 1024);
        assert!(parser.next().is_none());
        parser.advance(b"\x1b[201~z", false);
        assert!(matches!(parser.next(), Some(Ok(InternalEvent::Event(Event::Paste(text)))) if text.is_empty()));
        assert!(matches!(parser.next(), Some(Ok(InternalEvent::Event(Event::Key(key)))) if key.code == crate::event::KeyCode::Char('z')));
        assert!(parser.next().is_none());
    }

    #[test]
    fn paste_terminator_can_cross_the_buffer_limit() {
        for matched in 0..=6 {
            let mut parser = Parser::default();
            let mut text = b"\x1b[200~".to_vec();
            text.resize(128 * 1024 + 1 - matched, b'a');
            text.extend_from_slice(b"\x1b[201~z");
            parser.advance(&text, false);
            assert!(matches!(parser.next(), Some(Ok(InternalEvent::Event(Event::Paste(_))))));
            assert!(matches!(parser.next(), Some(Ok(InternalEvent::Event(Event::Key(key)))) if key.code == crate::event::KeyCode::Char('z')));
            assert!(parser.next().is_none());
        }
    }

    #[test]
    fn oversized_unfinished_escape_is_one_error_without_text_leakage() {
        let mut parser = Parser::default();
        parser.advance(b"\x1b[", true);
        for _ in 0..256 { parser.advance(&[b'1'; 1024], true); }
        assert!(parser.buffer.capacity() <= 256 * 1024);
        parser.advance(b"uz", false);
        assert!(matches!(parser.next(), Some(Err(error)) if error.kind() == io::ErrorKind::InvalidData));
        assert!(matches!(parser.next(), Some(Ok(InternalEvent::Event(Event::Key(key)))) if key.code == crate::event::KeyCode::Char('z')));
        assert!(parser.next().is_none());
    }
}
