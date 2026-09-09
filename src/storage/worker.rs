use super::{
    Best, BestOutcome, ErrorKind, Filter, HistoryPage, Options, Page, ReadStore, Record, Review,
    Statistics, StorageError, database::Writer,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};

pub type Ticket = u64;
pub type RequestId = u64;
const JOB_CAPACITY: usize = 16;
const REPLY_CAPACITY: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingState {
    Queued,
    InFlight,
    Unsaved(StorageError),
}
struct Pending {
    ticket: Ticket,
    record: Arc<Record>,
    state: PendingState,
}
#[derive(Debug)]
pub struct RejectedRecord {
    pub record: Record,
    pub error: StorageError,
}
#[derive(Debug, Clone)]
pub enum Event {
    Ready {
        journal_mode: String,
    },
    Unavailable(StorageError),
    Saved {
        ticket: Ticket,
        id: String,
        best: BestOutcome,
    },
    Unsaved {
        ticket: Ticket,
        error: StorageError,
    },
    History {
        request_id: RequestId,
        page: HistoryPage,
    },
    Stats {
        request_id: RequestId,
        statistics: Statistics,
    },
    Best {
        request_id: RequestId,
        best: Option<Best>,
    },
    Review {
        request_id: RequestId,
        review: Option<Box<Review>>,
    },
    QueryFailed {
        request_id: RequestId,
        error: StorageError,
    },
}
enum Job {
    Reconfigure(Options),
    Save {
        ticket: Ticket,
        record: Arc<Record>,
    },
    History {
        request_id: RequestId,
        filter: Filter,
        page: Page,
    },
    Stats {
        request_id: RequestId,
        filter: Filter,
    },
    Best {
        request_id: RequestId,
        profile: String,
    },
    Review {
        request_id: RequestId,
        id: String,
    },
}
impl Job {
    fn failed(self, error: StorageError) -> Event {
        match self {
            Self::Reconfigure(_) => Event::Unavailable(error),
            Self::Save { ticket, .. } => Event::Unsaved { ticket, error },
            Self::History { request_id, .. }
            | Self::Stats { request_id, .. }
            | Self::Best { request_id, .. }
            | Self::Review { request_id, .. } => Event::QueryFailed { request_id, error },
        }
    }
}
#[derive(Debug)]
pub struct FlushReport {
    pub acknowledged_during_flush: usize,
    pub unsaved: Vec<Arc<Record>>,
    pub worker_stopped: bool,
}

/// Frontend ownership is deliberately independent of SQLite connection ownership.
/// Calls other than the explicitly bounded flush do not wait for worker progress.
pub struct Store {
    jobs: Option<SyncSender<Job>>,
    replies: Option<Receiver<Event>>,
    join: Option<JoinHandle<()>>,
    pending: VecDeque<Pending>,
    pending_limit: usize,
    next_id: u64,
    read_only: bool,
}
impl Store {
    pub fn start_after_frame(options: Options, wake: Thread) -> Result<Self, StorageError> {
        options.validate()?;
        let pending_limit = options.pending_limit;
        let read_only = options.read_only;
        let (jobs, receiver) = mpsc::sync_channel(JOB_CAPACITY);
        let (sender, replies) = mpsc::sync_channel(REPLY_CAPACITY);
        let join = thread::Builder::new()
            .name("clack-storage".into())
            .spawn(move || worker(options, receiver, sender, wake))
            .map_err(StorageError::io)?;
        Ok(Self {
            jobs: Some(jobs),
            replies: Some(replies),
            join: Some(join),
            pending: VecDeque::with_capacity(pending_limit),
            pending_limit,
            next_id: 1,
            read_only,
        })
    }
    pub fn submit(&mut self, record: Record) -> Result<Ticket, Box<RejectedRecord>> {
        if self.read_only {
            return Err(Box::new(RejectedRecord {
                record,
                error: StorageError::new(
                    ErrorKind::ReadOnly,
                    "history worker is read-only; result was not submitted",
                ),
            }));
        }
        if self.pending.len() >= self.pending_limit {
            return Err(Box::new(RejectedRecord {
                record,
                error: StorageError::new(
                    ErrorKind::Capacity,
                    "pending result capacity reached; current result is unsaved and can be exported",
                ),
            }));
        }
        if let Err(error) = record.validate() {
            return Err(Box::new(RejectedRecord { record, error }));
        }
        let ticket = self.id();
        self.pending.push_back(Pending {
            ticket,
            record: Arc::new(record),
            state: PendingState::Queued,
        });
        self.dispatch();
        Ok(ticket)
    }
    pub fn pending(&self) -> impl Iterator<Item = (Ticket, &Record, &PendingState)> {
        self.pending
            .iter()
            .map(|pending| (pending.ticket, pending.record.as_ref(), &pending.state))
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    /// Reopen storage on its existing worker, after previously dispatched saves.
    /// The caller applies configuration changes outside active typing.
    pub fn reconfigure(&mut self, options: Options) -> Result<(), StorageError> {
        options.validate()?;
        self.dispatch();
        if self
            .pending
            .iter()
            .any(|pending| pending.state == PendingState::Queued)
        {
            return Err(StorageError::new(
                ErrorKind::Busy,
                "pending results must enter the worker queue before reconfiguration; retry the action",
            ));
        }
        let read_only = options.read_only;
        let pending_limit = options.pending_limit;
        self.send(Job::Reconfigure(options))?;
        self.read_only = read_only;
        self.pending_limit = pending_limit;
        Ok(())
    }
    pub fn retry_unsaved(&mut self) {
        for pending in &mut self.pending {
            if matches!(pending.state, PendingState::Unsaved(_)) {
                pending.state = PendingState::Queued;
            }
        }
        self.dispatch();
    }
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..REPLY_CAPACITY {
            let Some(replies) = &self.replies else {
                break;
            };
            match replies.try_recv() {
                Ok(event) => {
                    match &event {
                        Event::Saved { ticket, .. } => {
                            self.pending.retain(|pending| pending.ticket != *ticket)
                        }
                        Event::Unsaved { ticket, error } => {
                            if let Some(pending) = self
                                .pending
                                .iter_mut()
                                .find(|pending| pending.ticket == *ticket)
                            {
                                pending.state = PendingState::Unsaved(error.clone());
                            }
                        }
                        _ => {}
                    }
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    for pending in &mut self.pending {
                        if !matches!(pending.state, PendingState::Unsaved(_)) {
                            pending.state = PendingState::Unsaved(StorageError::new(
                                ErrorKind::Unavailable,
                                "storage worker stopped before save acknowledgement",
                            ));
                        }
                    }
                    break;
                }
            }
        }
        self.dispatch();
        events
    }
    pub fn request_history(
        &mut self,
        filter: Filter,
        page: Page,
    ) -> Result<RequestId, StorageError> {
        filter.validate()?;
        page.validate()?;
        let request_id = self.id();
        self.send(Job::History {
            request_id,
            filter,
            page,
        })?;
        Ok(request_id)
    }
    pub fn request_stats(&mut self, filter: Filter) -> Result<RequestId, StorageError> {
        filter.validate()?;
        let request_id = self.id();
        self.send(Job::Stats { request_id, filter })?;
        Ok(request_id)
    }
    pub fn request_best(&mut self, profile: impl Into<String>) -> Result<RequestId, StorageError> {
        let profile = profile.into();
        Filter::current(&profile).validate()?;
        let request_id = self.id();
        self.send(Job::Best {
            request_id,
            profile,
        })?;
        Ok(request_id)
    }
    pub fn request_review(&mut self, id: impl Into<String>) -> Result<RequestId, StorageError> {
        let id = id.into();
        super::model::validate_result_id(&id)?;
        let request_id = self.id();
        self.send(Job::Review { request_id, id })?;
        Ok(request_id)
    }
    /// Normal exit: <=750 ms. Interruption: <=100 ms. Caller selects the budget.
    /// A stalled fsync may outlive the budget; unacknowledged records stay unsaved.
    pub fn flush_for(&mut self, timeout: Duration) -> FlushReport {
        let deadline = Instant::now() + timeout.min(Duration::from_millis(750));
        self.retry_unsaved();
        let before = self.pending.len();
        loop {
            self.poll();
            if self.pending.is_empty()
                || Instant::now() >= deadline
                || self
                    .pending
                    .iter()
                    .all(|pending| matches!(pending.state, PendingState::Unsaved(_)))
            {
                break;
            }
            thread::park_timeout(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(5)),
            );
        }
        FlushReport {
            acknowledged_during_flush: before - self.pending.len(),
            unsaved: self
                .pending
                .iter()
                .map(|pending| Arc::clone(&pending.record))
                .collect(),
            worker_stopped: self.join.as_ref().is_none_or(JoinHandle::is_finished),
        }
    }
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("storage request ID space exhausted");
        id
    }
    fn send(&self, job: Job) -> Result<(), StorageError> {
        self.jobs
            .as_ref()
            .ok_or_else(|| StorageError::new(ErrorKind::Unavailable, "storage worker is closed"))?
            .try_send(job)
            .map_err(|error| match error {
                TrySendError::Full(_) => StorageError::new(
                    ErrorKind::Busy,
                    "storage request queue is busy; retry the action",
                ),
                TrySendError::Disconnected(_) => {
                    StorageError::new(ErrorKind::Unavailable, "storage worker is unavailable")
                }
            })
    }
    fn dispatch(&mut self) {
        let Some(jobs) = &self.jobs else {
            return;
        };
        for pending in &mut self.pending {
            if pending.state != PendingState::Queued {
                continue;
            }
            match jobs.try_send(Job::Save {
                ticket: pending.ticket,
                record: Arc::clone(&pending.record),
            }) {
                Ok(()) => pending.state = PendingState::InFlight,
                Err(TrySendError::Full(_)) => break,
                Err(TrySendError::Disconnected(_)) => {
                    pending.state = PendingState::Unsaved(StorageError::new(
                        ErrorKind::Unavailable,
                        "storage worker stopped before save acknowledgement",
                    ))
                }
            }
        }
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        self.jobs.take();
        self.replies.take();
        if let Some(join) = self.join.take() {
            if join.is_finished() {
                let _ = join.join();
            } else {
                drop(join);
            }
        }
    }
}
enum Access {
    Read(ReadStore),
    Write(Writer),
}
impl Access {
    fn open(options: &Options) -> Result<Self, StorageError> {
        if options.read_only {
            ReadStore::open(&options.database).map(Self::Read)
        } else {
            Writer::open(options).map(Self::Write)
        }
    }
    fn journal(&self) -> String {
        match self {
            Self::Read(_) => "read_only".into(),
            Self::Write(writer) => writer.journal.clone(),
        }
    }
    fn reader(&self) -> &ReadStore {
        match self {
            Self::Read(reader) => reader,
            Self::Write(writer) => &writer.reader,
        }
    }
    fn refresh_missing(&mut self, options: &Options) -> Result<(), StorageError> {
        if let Self::Read(reader) = self
            && reader.connection.is_none()
        {
            // Another process may create history after this read-only
            // worker starts. Retrying on an explicit query stays entirely
            // on the worker and does not create directories or a database.
            *reader = ReadStore::open(&options.database)?;
        }
        Ok(())
    }
    fn save(&mut self, record: &Record) -> Result<BestOutcome, StorageError> {
        match self {
            Self::Read(_) => Err(StorageError::new(
                ErrorKind::ReadOnly,
                "history worker is read-only; no result was saved",
            )),
            Self::Write(writer) => writer.save(record),
        }
    }
}
fn worker(mut options: Options, jobs: Receiver<Job>, replies: SyncSender<Event>, wake: Thread) {
    let notify = |event| {
        let sent = replies.send(event).is_ok();
        wake.unpark();
        sent
    };
    let mut writer = match Access::open(&options) {
        Ok(writer) => {
            if !notify(Event::Ready {
                journal_mode: writer.journal(),
            }) {
                return;
            }
            Some(writer)
        }
        Err(error) => {
            if !notify(Event::Unavailable(error)) {
                return;
            }
            None
        }
    };
    while let Ok(job) = jobs.recv() {
        if let Job::Reconfigure(next_options) = &job {
            // Closing first also checkpoints the old WAL before selecting a new
            // journal mode. No second writer or blocking foreground join exists.
            drop(writer.take());
            options = next_options.clone();
            match Access::open(&options) {
                Ok(reopened) => {
                    if !notify(Event::Ready {
                        journal_mode: reopened.journal(),
                    }) {
                        return;
                    }
                    writer = Some(reopened);
                }
                Err(error) => {
                    if !notify(Event::Unavailable(error)) {
                        return;
                    }
                }
            }
            continue;
        }
        if writer.is_none() {
            match Access::open(&options) {
                Ok(reopened) => {
                    if !notify(Event::Ready {
                        journal_mode: reopened.journal(),
                    }) {
                        return;
                    }
                    writer = Some(reopened);
                }
                Err(error) => {
                    if !notify(job.failed(error)) {
                        return;
                    }
                    continue;
                }
            }
        }
        let writer = writer.as_mut().expect("opened storage worker");
        if let Err(error) = writer.refresh_missing(&options) {
            if !notify(job.failed(error)) {
                return;
            }
            continue;
        }
        let event = match job {
            Job::Reconfigure(_) => unreachable!("handled before access opening"),
            Job::Save { ticket, record } => match writer.save(&record) {
                Ok(best) => Event::Saved {
                    ticket,
                    id: record.id().into(),
                    best,
                },
                Err(error) => Event::Unsaved { ticket, error },
            },
            Job::History {
                request_id,
                filter,
                page,
            } => match writer.reader().history(&filter, page) {
                Ok(page) => Event::History { request_id, page },
                Err(error) => Event::QueryFailed { request_id, error },
            },
            Job::Stats { request_id, filter } => match writer.reader().stats(&filter) {
                Ok(statistics) => Event::Stats {
                    request_id,
                    statistics,
                },
                Err(error) => Event::QueryFailed { request_id, error },
            },
            Job::Best {
                request_id,
                profile,
            } => match writer.reader().best(&profile) {
                Ok(best) => Event::Best { request_id, best },
                Err(error) => Event::QueryFailed { request_id, error },
            },
            Job::Review { request_id, id } => match writer.reader().review(&id) {
                Ok(review) => Event::Review {
                    request_id,
                    review: review.map(Box::new),
                },
                Err(error) => Event::QueryFailed { request_id, error },
            },
        };
        if !notify(event) {
            return;
        }
    }
}
