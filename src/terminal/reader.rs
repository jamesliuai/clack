use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, PollWaker};
use unicode_segmentation::UnicodeSegmentation;

use crate::content::validate_input_scalar;
use crate::engine::{Mode, Policy, eligible_text_start};
use crate::settings::{BindingContext, Bindings};

pub const INPUT_CAPACITY: usize = 4096;
const NO_OVERLOAD: u64 = u64::MAX;
const IDLE_WAIT: Duration = Duration::from_secs(86_400);

#[derive(Debug, Clone)]
pub struct ReaderOptions {
    pub mode: Mode,
    pub policy: Policy,
    pub seconds: u32,
    pub bindings: Arc<Bindings>,
}

impl Default for ReaderOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Time,
            policy: Policy::Prose,
            seconds: 30,
            bindings: Arc::new(Bindings::default()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum InputKind {
    Key {
        key: KeyEvent,
        associated_text: Option<String>,
    },
    Paste,
    Resize(u16, u16),
    FocusLost,
    Tick,
    EpochReady,
    EpochClosed {
        closed_epoch: u64,
        overloaded: bool,
    },
    Signal(i32),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct Envelope {
    pub epoch: u64,
    pub sequence: u64,
    pub received_us: u64,
    pub kind: InputKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochClosure {
    pub closed_epoch: u64,
    pub overloaded: bool,
}

#[derive(Debug, Clone)]
pub enum Control {
    Restart { epoch: u64, options: ReaderOptions },
    Disarm { epoch: u64 },
    Stop,
}

struct Shared {
    queue: Mutex<VecDeque<Envelope>>,
    closed: AtomicBool,
    overload_epoch: AtomicU64,
    last_closed: Mutex<Option<EpochClosure>>,
    notify: Thread,
}

/// One bounded queue; only resize notifications are coalesced. Text events are
/// neither overwritten nor merged. TryRecvError matches std channel call sites.
pub struct EventReceiver {
    shared: Arc<Shared>,
}

impl EventReceiver {
    /// Current bounded queue occupancy, including control acknowledgements.
    pub fn len(&self) -> usize {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn try_recv(&self) -> Result<Envelope, TryRecvError> {
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        queue.pop_front().ok_or_else(|| {
            if self.shared.closed.load(Ordering::Acquire) {
                TryRecvError::Disconnected
            } else {
                TryRecvError::Empty
            }
        })
    }
}

impl Shared {
    fn push(&self, mut envelope: Envelope) -> bool {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if matches!(envelope.kind, InputKind::Resize(width, height) if width >= 40 && height >= 10)
        {
            queue.retain(|event| !matches!(event.kind, InputKind::Resize(width, height) if width >= 40 && height >= 10));
        }
        if matches!(envelope.kind, InputKind::Resize(width, height) if width < 40 || height < 10) {
            // Keep the earliest interruption in stream order and the latest
            // geometry, even during a resize flood. No input text is coalesced.
            let mut kept_first = false;
            queue.retain(|event| {
                if matches!(event.kind, InputKind::Resize(width, height) if width < 40 || height < 10) {
                    if kept_first { false } else { kept_first = true; true }
                } else { true }
            });
        }
        if let InputKind::Signal(signal) = envelope.kind {
            // Standard process signals are already coalescing OS notices.
            if queue
                .iter()
                .any(|event| matches!(event.kind, InputKind::Signal(prior) if prior == signal))
            {
                return true;
            }
        }
        if let InputKind::EpochClosed {
            closed_epoch,
            overloaded,
        } = &mut envelope.kind
        {
            *overloaded |= queue.len() == INPUT_CAPACITY;
            *self
                .last_closed
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = Some(EpochClosure {
                closed_epoch: *closed_epoch,
                overloaded: *overloaded,
            });
        }
        if queue.len() == INPUT_CAPACITY {
            self.overload_epoch.store(envelope.epoch, Ordering::Release);
            // Critical lifecycle packets remain deliverable after overflow.
            // Removing a scored event is explicit: the sticky verdict already
            // invalidates this epoch, and the sequence gap remains observable.
            if matches!(
                envelope.kind,
                InputKind::Signal(_) | InputKind::Resize(..) | InputKind::EpochClosed { .. }
            ) {
                let index = queue
                    .iter()
                    .rposition(|event| {
                        !matches!(
                            event.kind,
                            InputKind::Signal(_)
                                | InputKind::Resize(..)
                                | InputKind::EpochClosed { .. }
                        )
                    })
                    .or_else(|| {
                        queue
                            .iter()
                            .position(|event| matches!(event.kind, InputKind::EpochClosed { .. }))
                    })
                    .unwrap_or(0);
                queue.remove(index);
                queue.push_back(envelope);
            }
            drop(queue);
            self.notify.unpark();
            return false;
        }
        queue.push_back(envelope);
        drop(queue);
        self.notify.unpark();
        true
    }

    fn barrier(&self, epoch: u64) {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // Process signals and geometry remain meaningful across test epochs.
        queue.retain_mut(|event| match event.kind {
            InputKind::Signal(_) | InputKind::EpochClosed { .. } => true,
            InputKind::Resize(..) => {
                event.epoch = epoch;
                true
            }
            _ => false,
        });
    }
}

pub struct Reader {
    pub events: EventReceiver,
    controls: SyncSender<Control>,
    waker: PollWaker,
    worker: Option<JoinHandle<()>>,
}

impl Reader {
    pub fn start(
        options: ReaderOptions,
        epoch: u64,
        origin: Instant,
        notify: Thread,
    ) -> io::Result<Self> {
        if epoch == NO_OVERLOAD || !(1..=3600).contains(&options.seconds) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid input-reader epoch or duration",
            ));
        }
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::with_capacity(INPUT_CAPACITY)),
            closed: AtomicBool::new(false),
            overload_epoch: AtomicU64::new(NO_OVERLOAD),
            last_closed: Mutex::new(None),
            notify,
        });
        let (controls, receive_controls) = mpsc::sync_channel(32);
        let (initialized, ready) = mpsc::sync_channel(1);
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("clack-input".into())
            .spawn(move || {
                let waker = match event::poll_waker() {
                    Ok(waker) => waker,
                    Err(error) => {
                        let _ = initialized.send(Err(error));
                        worker_shared.closed.store(true, Ordering::Release);
                        worker_shared.notify.unpark();
                        return;
                    }
                };
                if initialized.send(Ok(waker)).is_ok() {
                    let mut state =
                        InputOwner::new(options, epoch, origin, Arc::clone(&worker_shared));
                    state.run(receive_controls);
                    let already_closed = worker_shared
                        .last_closed
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .is_some_and(|closure| closure.closed_epoch == state.epoch);
                    if !already_closed {
                        state.disarm();
                        let overloaded =
                            worker_shared.overload_epoch.load(Ordering::Acquire) == state.epoch;
                        state.close_epoch(state.epoch, overloaded);
                    }
                }
                worker_shared.closed.store(true, Ordering::Release);
                worker_shared.notify.unpark();
            })?;
        match ready.recv() {
            Ok(Ok(waker)) => Ok(Self {
                events: EventReceiver { shared },
                controls,
                waker,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(io::Error::other(
                    "input reader stopped during initialization",
                ))
            }
        }
    }

    pub fn command(&self, control: Control) -> io::Result<()> {
        let valid = match &control {
            Control::Restart { epoch, options } => {
                *epoch != NO_OVERLOAD && (1..=3600).contains(&options.seconds)
            }
            Control::Disarm { epoch } => *epoch != NO_OVERLOAD,
            Control::Stop => true,
        };
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid input-reader epoch or duration",
            ));
        }
        self.controls
            .try_send(control)
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    io::Error::new(io::ErrorKind::WouldBlock, "input control queue is full")
                }
                TrySendError::Disconnected(_) => {
                    io::Error::new(io::ErrorKind::BrokenPipe, "input reader has stopped")
                }
            })?;
        self.waker.wake()
    }

    pub fn overload_epoch(&self) -> Option<u64> {
        let epoch = self.events.shared.overload_epoch.load(Ordering::Acquire);
        (epoch != NO_OVERLOAD).then_some(epoch)
    }

    /// The latest closure verdict remains available after shutdown. Callers
    /// permit only one outstanding captured result; the ordered packet is the
    /// normal path and this bounded slot covers suspension/shutdown recovery.
    pub fn last_closed(&self) -> Option<EpochClosure> {
        *self
            .events
            .shared
            .last_closed
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Stop the only input owner before restoring terminal capabilities.
    pub fn shutdown(&mut self) -> io::Result<()> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        // A stop does not compete with a full command queue: wake first, then the
        // bounded send can progress as the input owner drains its controls.
        let _ = self.waker.wake();
        let _ = self.controls.send(Control::Stop);
        let _ = self.waker.wake();
        worker
            .join()
            .map_err(|_| io::Error::other("input reader panicked"))
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Preserve text separately from metadata and reject a release before it can
/// start a run. Atomic paste never becomes a sequence of Text events.
pub fn normalize_event(event: Event) -> Result<Option<InputKind>, String> {
    match event {
        Event::Key(key) => {
            if key.kind == KeyEventKind::Release {
                return Ok(None);
            }
            if let KeyCode::Char(character) = key.code {
                validate_input_scalar(character).map_err(|error| error.to_string())?;
            }
            Ok(Some(InputKind::Key {
                key,
                associated_text: None,
            }))
        }
        Event::KeyWithText { key, text } => {
            if key.kind == KeyEventKind::Release {
                return Ok(None);
            }
            if text.len() > 16 * 1024 {
                return Err("associated keyboard text exceeds 16 KiB".into());
            }
            let mut graphemes = 0;
            for grapheme in text.graphemes(true) {
                graphemes += 1;
                if graphemes > 128 {
                    return Err("associated keyboard text exceeds 128 graphemes".into());
                }
                if grapheme.chars().count() > 32 {
                    return Err("input grapheme exceeds 32 Unicode scalars".into());
                }
            }
            for character in text.chars() {
                validate_input_scalar(character).map_err(|error| error.to_string())?;
            }
            Ok(Some(InputKind::Key {
                key,
                associated_text: Some(text),
            }))
        }
        Event::Paste(_) => Ok(Some(InputKind::Paste)),
        Event::Resize(width, height) => Ok(Some(InputKind::Resize(width, height))),
        Event::FocusLost => Ok(Some(InputKind::FocusLost)),
        #[cfg(unix)]
        Event::Signal(signal) => Ok(Some(InputKind::Signal(signal))),
        Event::FocusGained | Event::Mouse(_) => Ok(None),
    }
}

impl InputKind {
    pub fn starts_test(&self, options: &ReaderOptions) -> bool {
        let Self::Key {
            key,
            associated_text,
        } = self
        else {
            return false;
        };
        if options
            .bindings
            .resolve(key, BindingContext::Running)
            .is_some()
        {
            return false;
        }
        if let Some(text) = associated_text {
            return eligible_text_start(options.mode, options.policy, text);
        }
        if key.modifiers.intersects(
            KeyModifiers::CONTROL
                | KeyModifiers::ALT
                | KeyModifiers::SUPER
                | KeyModifiers::HYPER
                | KeyModifiers::META,
        ) {
            return false;
        }
        let mut bytes = [0; 4];
        let text = match key.code {
            KeyCode::Char(character) => character.encode_utf8(&mut bytes),
            KeyCode::Tab => "\t",
            KeyCode::Enter => "\n",
            _ => return false,
        };
        eligible_text_start(options.mode, options.policy, text)
    }
}

struct InputOwner {
    options: ReaderOptions,
    epoch: u64,
    sequence: u64,
    origin: Instant,
    armed: bool,
    started: Option<u64>,
    next_tick: Option<u64>,
    deadline: Option<u64>,
    last_observed_us: u64,
    last_wall: SystemTime,
    shared: Arc<Shared>,
}

impl InputOwner {
    fn new(options: ReaderOptions, epoch: u64, origin: Instant, shared: Arc<Shared>) -> Self {
        Self {
            options,
            epoch,
            sequence: 0,
            origin,
            armed: true,
            started: None,
            next_tick: None,
            deadline: None,
            last_observed_us: 0,
            last_wall: SystemTime::now(),
            shared,
        }
    }

    fn now(&self) -> u64 {
        self.origin.elapsed().as_micros().min(u64::MAX as u128) as u64
    }

    fn emit(&mut self, kind: InputKind, at: u64) -> bool {
        self.sequence = self.sequence.saturating_add(1);
        self.shared.push(Envelope {
            epoch: self.epoch,
            sequence: self.sequence,
            received_us: at,
            kind,
        })
    }

    fn disarm(&mut self) {
        self.armed = false;
        self.started = None;
        self.next_tick = None;
        self.deadline = None;
    }

    fn control(&mut self, control: Control) -> bool {
        let closed_epoch = self.epoch;
        let (epoch, options, armed, stop) = match control {
            Control::Stop => (self.epoch, self.options.clone(), false, true),
            Control::Restart { epoch, options } => (epoch, options, true, false),
            Control::Disarm { epoch } => (epoch, self.options.clone(), false, false),
        };
        self.disarm();
        self.shared.barrier(epoch);
        // The sole producer captures the old sticky verdict before reusing its
        // epoch slot. No later input can be stamped with the closed epoch.
        let prior_overload = self
            .shared
            .overload_epoch
            .swap(NO_OVERLOAD, Ordering::AcqRel)
            == closed_epoch;
        self.epoch = epoch;
        self.options = options;
        let mut ready = false;
        let keep_running = (|| {
            if !self.discard_pending() {
                return !stop && self.shared.overload_epoch.load(Ordering::Acquire) == epoch;
            }
            if stop {
                return false;
            }
            // Drop old complete events and partial terminal bytes before the
            // closure verdict and acknowledgement, retaining global signals
            // and geometry throughout the barrier.
            for drained in 0..=INPUT_CAPACITY {
                match event::poll(Duration::ZERO) {
                    Ok(false) => {
                        if !self.discard_pending() {
                            return self.shared.overload_epoch.load(Ordering::Acquire) == epoch;
                        }
                        self.armed = armed;
                        ready = true;
                        return true;
                    }
                    Ok(true) => {
                        if drained == INPUT_CAPACITY {
                            self.shared.overload_epoch.store(epoch, Ordering::Release);
                            self.shared.notify.unpark();
                            return true;
                        }
                        match event::read() {
                            #[cfg(unix)]
                            Ok(Event::Signal(signal)) => {
                                self.emit(InputKind::Signal(signal), self.now());
                            }
                            Ok(Event::Resize(width, height)) => {
                                self.emit(InputKind::Resize(width, height), self.now());
                            }
                            Ok(_) => {}
                            Err(error) => {
                                let recoverable = error.kind() == io::ErrorKind::InvalidData;
                                self.emit(
                                    InputKind::Error(format!(
                                        "input interruption while restarting: {error}"
                                    )),
                                    self.now(),
                                );
                                return recoverable;
                            }
                        }
                    }
                    Err(error) => {
                        let recoverable = error.kind() == io::ErrorKind::InvalidData;
                        self.emit(
                            InputKind::Error(format!(
                                "input interruption while restarting: {error}"
                            )),
                            self.now(),
                        );
                        return recoverable;
                    }
                }
            }
            true
        })();
        let overloaded =
            prior_overload || self.shared.overload_epoch.load(Ordering::Acquire) == epoch;
        self.close_epoch(closed_epoch, overloaded);
        if ready {
            self.emit(InputKind::EpochReady, self.now());
        }
        keep_running
    }

    fn close_epoch(&mut self, closed_epoch: u64, overloaded: bool) {
        self.emit(
            InputKind::EpochClosed {
                closed_epoch,
                overloaded,
            },
            self.now(),
        );
    }

    fn discard_pending(&mut self) -> bool {
        match event::discard_pending_input() {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                self.shared
                    .overload_epoch
                    .store(self.epoch, Ordering::Release);
                self.shared.notify.unpark();
                false
            }
            Err(error) => {
                self.emit(
                    InputKind::Error(format!("cannot discard stale terminal input: {error}")),
                    self.now(),
                );
                false
            }
        }
    }

    fn observe_clock(&mut self, now: u64) -> bool {
        let wall = SystemTime::now();
        let monotonic_delta = now.saturating_sub(self.last_observed_us);
        let wall_delta = wall
            .duration_since(self.last_wall)
            .ok()
            .map(|delta| delta.as_micros().min(u64::MAX as u128) as u64);
        let interrupted = self.started.is_some()
            && (now < self.last_observed_us
                || monotonic_delta > 3_000_000
                || wall_delta.is_none_or(|delta| delta.abs_diff(monotonic_delta) > 1_000_000));
        self.last_observed_us = now;
        self.last_wall = wall;
        if interrupted {
            self.disarm();
            self.emit(
                InputKind::Error("clock or suspend interruption invalidated the active run".into()),
                now,
            );
        }
        !interrupted
    }

    fn watermarks(&mut self, now: u64) {
        while let Some(tick) = self.next_tick {
            if tick > now {
                break;
            }
            if !self.emit(InputKind::Tick, tick) {
                self.disarm();
                break;
            }
            if self.deadline.is_some_and(|deadline| tick >= deadline) {
                self.disarm();
                break;
            }
            self.next_tick = Some(tick + 1_000_000);
        }
    }

    fn run(&mut self, controls: Receiver<Control>) {
        loop {
            while let Ok(control) = controls.try_recv() {
                if !self.control(control) {
                    return;
                }
            }
            if self.shared.overload_epoch.load(Ordering::Acquire) != NO_OVERLOAD {
                match controls.recv() {
                    Ok(control) => {
                        if !self.control(control) {
                            return;
                        }
                        continue;
                    }
                    Err(_) => return,
                }
            }
            let now = self.now();
            self.observe_clock(now);
            self.watermarks(now);
            let timeout = self.next_tick.map_or(IDLE_WAIT, |at| {
                Duration::from_micros(at.saturating_sub(self.now()))
            });
            match event::poll(timeout) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(error) => {
                    let recoverable = error.kind() == io::ErrorKind::InvalidData;
                    self.disarm();
                    self.emit(
                        InputKind::Error(format!("terminal input interruption: {error}")),
                        self.now(),
                    );
                    if !recoverable {
                        return;
                    }
                    continue;
                }
            }
            let received = event::read();
            let permanent_error = received
                .as_ref()
                .is_err_and(|error| error.kind() != io::ErrorKind::InvalidData);
            let at = self.now();
            self.observe_clock(at);
            // A read can complete at a deadline. Its prior watermark is emitted
            // first, and the late event is never treated as pre-deadline input.
            self.watermarks(at);
            let kind = match received
                .map_err(|error| error.to_string())
                .and_then(normalize_event)
            {
                Ok(Some(kind)) => kind,
                Ok(None) => continue,
                Err(error) => {
                    self.disarm();
                    self.emit(
                        InputKind::Error(format!("terminal input interruption: {error}")),
                        at,
                    );
                    if permanent_error {
                        return;
                    }
                    continue;
                }
            };
            if self.armed && self.started.is_none() && kind.starts_test(&self.options) {
                self.started = Some(at);
                self.next_tick = Some(at + 1_000_000);
                self.deadline = (self.options.mode == Mode::Time)
                    .then_some(at + u64::from(self.options.seconds) * 1_000_000);
                self.last_observed_us = at;
                self.last_wall = SystemTime::now();
            }
            if !self.emit(kind, at) {
                self.disarm();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            queue: Mutex::new(VecDeque::with_capacity(INPUT_CAPACITY)),
            closed: AtomicBool::new(false),
            overload_epoch: AtomicU64::new(NO_OVERLOAD),
            last_closed: Mutex::new(None),
            notify: thread::current(),
        })
    }

    fn envelope(sequence: u64, kind: InputKind) -> Envelope {
        Envelope {
            epoch: 3,
            sequence,
            received_us: sequence,
            kind,
        }
    }

    #[test]
    fn event_queue_has_a_hard_bound_and_explicit_sticky_overload() {
        let shared = shared();
        for sequence in 0..INPUT_CAPACITY as u64 {
            assert!(shared.push(envelope(
                sequence,
                InputKind::Key {
                    key: KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
                    associated_text: None,
                }
            )));
        }
        assert!(!shared.push(envelope(INPUT_CAPACITY as u64, InputKind::Tick)));
        assert_eq!(shared.overload_epoch.load(Ordering::Acquire), 3);
        let receiver = EventReceiver { shared };
        assert_eq!(receiver.len(), INPUT_CAPACITY);
        assert!(!receiver.is_empty());
        for sequence in 0..INPUT_CAPACITY as u64 {
            assert_eq!(receiver.try_recv().unwrap().sequence, sequence);
        }
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        assert!(receiver.is_empty());
        assert_eq!(receiver.shared.overload_epoch.load(Ordering::Acquire), 3);
    }

    #[test]
    fn resize_coalescing_preserves_text_order_and_small_geometry_transitions() {
        let shared = shared();
        shared.push(envelope(1, InputKind::Resize(80, 24)));
        shared.push(envelope(
            2,
            InputKind::Key {
                key: KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
                associated_text: None,
            },
        ));
        shared.push(envelope(3, InputKind::Resize(30, 8)));
        shared.push(envelope(4, InputKind::Resize(120, 40)));
        let receiver = EventReceiver { shared };
        assert_eq!(receiver.try_recv().unwrap().sequence, 2);
        assert!(matches!(
            receiver.try_recv().unwrap().kind,
            InputKind::Resize(30, 8)
        ));
        assert!(matches!(
            receiver.try_recv().unwrap().kind,
            InputKind::Resize(120, 40)
        ));
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn epoch_barrier_removes_old_text_but_preserves_global_process_signals() {
        let shared = shared();
        shared.push(envelope(
            1,
            InputKind::Key {
                key: KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
                associated_text: None,
            },
        ));
        shared.push(envelope(2, InputKind::Signal(15)));
        shared.push(envelope(3, InputKind::Tick));
        shared.push(envelope(4, InputKind::Resize(30, 8)));
        shared.barrier(4);
        let receiver = EventReceiver { shared };
        assert!(matches!(
            receiver.try_recv().unwrap().kind,
            InputKind::Signal(15)
        ));
        let resize = receiver.try_recv().unwrap();
        assert_eq!(resize.epoch, 4);
        assert!(matches!(resize.kind, InputKind::Resize(30, 8)));
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn reader_schedules_buckets_and_deadline_in_order_then_stops_ticking() {
        let shared = shared();
        let mut owner = InputOwner::new(
            ReaderOptions::default(),
            3,
            Instant::now(),
            Arc::clone(&shared),
        );
        owner.started = Some(123);
        owner.next_tick = Some(1_000_123);
        owner.deadline = Some(30_000_123);
        owner.watermarks(31_000_000);
        let receiver = EventReceiver { shared };
        for second in 1..=30 {
            let event = receiver.try_recv().unwrap();
            assert!(matches!(event.kind, InputKind::Tick));
            assert_eq!(event.received_us, second * 1_000_000 + 123);
        }
        owner.watermarks(60_000_000);
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        assert!(!owner.armed);
    }

    #[test]
    fn ready_and_disarmed_states_have_no_periodic_watermarks() {
        let shared = shared();
        let mut owner = InputOwner::new(
            ReaderOptions::default(),
            3,
            Instant::now(),
            Arc::clone(&shared),
        );
        owner.watermarks(86_400_000_000);
        owner.disarm();
        owner.watermarks(172_800_000_000);
        let receiver = EventReceiver { shared };
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn a_full_queue_cannot_hide_an_epoch_closure_or_claim_a_valid_verdict() {
        let shared = shared();
        for sequence in 0..INPUT_CAPACITY as u64 {
            shared.push(envelope(sequence, InputKind::Tick));
        }
        let mut owner = InputOwner::new(
            ReaderOptions::default(),
            4,
            Instant::now(),
            Arc::clone(&shared),
        );
        owner.sequence = INPUT_CAPACITY as u64;
        owner.close_epoch(3, false);
        let receiver = EventReceiver { shared };
        assert_eq!(receiver.len(), INPUT_CAPACITY);
        let mut closure = None;
        while let Ok(event) = receiver.try_recv() {
            if let InputKind::EpochClosed {
                closed_epoch,
                overloaded,
            } = event.kind
            {
                closure = Some((closed_epoch, overloaded));
            }
        }
        assert_eq!(closure, Some((3, true)));
        assert_eq!(
            *receiver.shared.last_closed.lock().unwrap(),
            Some(EpochClosure {
                closed_epoch: 3,
                overloaded: true
            })
        );
    }

    #[test]
    fn backwards_receipt_clock_interrupts_instead_of_clamping_elapsed_time() {
        let shared = shared();
        let mut owner = InputOwner::new(
            ReaderOptions::default(),
            3,
            Instant::now(),
            Arc::clone(&shared),
        );
        owner.started = Some(10);
        owner.last_observed_us = 100;
        assert!(!owner.observe_clock(99));
        assert!(!owner.armed);
        assert!(matches!(
            EventReceiver { shared }.try_recv().unwrap().kind,
            InputKind::Error(_)
        ));
    }
}
