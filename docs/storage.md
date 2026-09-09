# Local history and recovery

`clack` stores local SQLite summaries and one-second samples. Storage runs on one
worker thread, started only after the first complete usable frame. SQLite never
runs in the input reader or typing reducer. A result remains **unsaved** until its
transaction acknowledgement reaches the application. The interface can render and
start another test while a personal-best comparison or save is pending.

Actual SQLite acceptance tests cover the worker and export contracts; product PTY
tests exercise application history and recovery. Native platform validation is
tracked separately from these local backend and PTY checks.

## Application integration

Create the worker after the first successfully flushed frame, not before `Ready`:

```rust
use clack::storage::{Options, JournalMode, Store};
let mut options = Options::new(paths.database.clone());
options.pending_limit = config.storage.pending_limit;
options.journal = JournalMode::parse(&config.storage.journal_mode)?;
options.read_only = !config.privacy.save_results;
let mut store = Store::start_after_frame(options, std::thread::current())?;
```

`Options::new` defaults to 16 pending records, a 250 ms SQLite busy budget,
automatic journal selection, and a writable worker. `read_only=true` opens only
existing history and performs no initialization or migration. Missing history is
an empty database view. A later query retries the read-only open if another process
has since created the database, entirely on the worker. Private sessions can defer this read-only worker until an
explicit History action. `Store::reconfigure(options)` reopens the connection on
the same worker after all earlier dispatched saves. It validates and enqueues
before changing the facade's read-only policy/capacity. If earlier snapshots cannot
yet enter the bounded queue, it returns Busy so the caller can retry. Reducing the
pending limit retains existing records and refuses new ones until below the limit.
Apply storage settings outside active typing; successful reopening emits `Ready`.

At completion, pass the original immutable engine snapshot through the central
privacy boundary. Current review may continue using the engine's in-memory text.

```rust
use clack::storage::{Record, Persistence};
let privacy = Persistence {
    save_results: config.privacy.save_results,
    store_custom_text: config.privacy.store_custom_text,
    store_event_trace: config.privacy.store_event_trace,
};
if let Some(record) = Record::prepare(
    engine.snapshot(), privacy, Some(&sample.text), optional_trace,
)? {
    match store.submit(record) {
        Ok(ticket) => { /* retain ticket -> test epoch association; show unsaved */ }
        Err(rejected) => {
            // Ownership of rejected.record is returned. Keep the current result
            // available for export; never silently evict an older pending result.
        }
    }
}
```

`submit` is nonblocking and returns `Result<Ticket, Box<RejectedRecord>>`.
The facade retains at most the configured number of immutable `Arc<Record>`s;
the job channel has capacity 16 and the reply channel capacity 32. A full queue
does not lose an input event or overwrite a pending result. Request IDs and save
tickets share a process-local monotonic sequence. Durable record IDs remain stable
when the same record is retried, so acknowledgements lost during shutdown cannot
cause duplicate history/statistics on retry.

Drain `store.poll()` when its worker unparks the render owner. It returns bounded
`Event`s: `Ready`, `Unavailable`, `Saved`, `Unsaved`, `History`, `Stats`, `Best`,
`Review`, and `QueryFailed`. `Review.review` is an `Option<Box<Review>>`. Query
methods return request IDs: `request_history(Filter, Page)`,
`request_stats(Filter)`, `request_best(profile_key)`, and `request_review(id)`.
Match responses to the relevant epoch/request before updating a screen. A missing
or pending response means personal best is **unknown**. `Saved.best` distinguishes
`NewBest`, `NotBest`, `Tied`, `Ineligible`, and `AlreadySaved`; only `NewBest` after
acknowledgement supports a new local-personal-best announcement.

`retry_unsaved()` resubmits retained failures with their original record IDs.
`pending()` yields `(ticket, &Record, &PendingState)` for status/recovery. Normal
exit calls `flush_for(Duration::from_millis(750))`; interruption uses at most
100 ms. The flush returns the unacknowledged records. Restore the terminal, then
report their count and offer recovery export. `Drop` never unconditionally joins a
thread blocked in a filesystem call. An OS-level stalled sync can outlive the
frontend's timeout; those results cannot be described as acknowledged saves.
Forced termination and power loss can lose unacknowledged in-memory records.

## Independent history classification

`Filter.classification` is an optional `Classification` value, orthogonal to mode,
profile, date, source, and completion outcome. History pages, weighted statistics,
and streamed exports apply identical predicates:

- `standard`: the stored row is eligible for a local standard record.
- `practice`: explicit seed, repeated sample, named practice, numeric pace,
  auto-indent, paste attempt, Zen, or incomplete completion.
- `paste_attempted`: the saved paste-attempt flag is true.
- `assisted_code`: auto-indent was enabled for exact/code content, including an
  exact Custom run.

These sets intentionally overlap. A pasted assisted run matches the last three.
A plain failure/interruption has its own outcome and does not acquire a fabricated
practice reason. Likewise a zero-duration unassisted complete run has no valid
standard speed and no invented practice reason: it remains in unfiltered and
`complete` history, and matches neither `standard` nor `practice`.

New schema-1 databases add partial date/ID indexes for all four predicates. The
predicates read only neutral stored flags/conditions, never the private body or
trace. Existing valid schema-1 databases without the optional indexes still query
correctly; opening them never rewrites records or changes their recorded identity.
The current-profile materialized-summary shortcut is used only without a class
filter, so class-restricted statistics cannot accidentally return unfiltered sums.

## Privacy boundary and optional traces

Default records retain counts, effective conditions, versions, source hash, and
one-second samples. Known approved content may retain target word summaries, but
all entered strings are cleared. Custom/imported/zen word text is removed by
default. Custom and code identities use neutral labels, and zen uses `zen`.
Source file paths and CLI literal strings are not record fields. `save_results=false`
returns `None` before serialization/submission. Private-mode capture suppression is
also required at the application boundary, so private text never enters a trace.

`store_custom_text` and `store_event_trace` are independent opt-ins. Full custom
source admission is capped at 1 MiB of raw UTF-8, while the canonical body has a
separate 4 MiB bound because NFC can expand its representation. The pinned Unicode
17 decomposition proof and maximum-size expansion fixtures are described in
`docs/content.md`. Custom/code bodies must match the saved canonical content hash.
The `full_text` field is accompanied by `text_scope`: `complete_target` for fixed
passages, `prepared_target` for a generated sample, `complete_output` for Zen before
discarding output, or `retained_window` after Zen's editable window has cycled.
`Record::text_scope()` exposes the same value. An opted-in Zen window may contain
safe entered combining/joiner continuations; it is not validated as target prose.
Default exports remove both the body and scope.

Word summaries are capped at 65,536 and 24 MiB of serialized diagnostics with an
explicit `word_summaries_omitted` count. A canonical custom token and entered
diagnostic up to 4 MiB each can round-trip through storage; per-row and total
reader/writer bounds agree. An oversized optional diagnostic ends the retained
word-summary prefix and increases the omission count instead of losing the result.
The budget also reserves the body, trace, samples, and metadata before admitting
word diagnostics, so optional diagnostics cannot exceed the 32 MiB whole-record
bound. Entered summaries use scalar admission and a
32-scalar grapheme bound, allowing the same orphan continuations as live input.
The optional diagnostic trace API is:

```rust
TraceEvent { received_us: u64, sequence: u64, kind: TraceKind, text: Option<String> }
EventTrace { events: Vec<TraceEvent>, events_total: u64, truncated: bool }
```

`TraceKind` includes Text, Backspace, DeleteWord, Finish, PasteAttempt, FocusLost,
Abort, Interrupt, and Overload. Capture at most `TRACE_EVENT_LIMIT=8192` events,
each optional text field at most `TRACE_TEXT_BYTES=128` UTF-8 bytes. Advance
`events_total` after saturation and set `truncated=true`. Sequences must strictly
increase and receipt timestamps must not decrease. Only Text events can contain
text; never capture a rejected bracketed-paste payload. Validation permits safe
combining/joiner continuations while rejecting unsupported controls/bidi/shaping
scalars. No trace is written per key; the complete bounded record goes to the
worker after completion.

Default exports apply privacy filtering again, including records originally saved
with text enabled. `include_text=true` includes only text actually retained; it
cannot recover discarded text. A historical custom/code review without stored
source returns `OriginalRequired { content_hash }`. `Review::attach_original(bytes)`
validates the original under the saved policy and normalization before matching
its hash. Correct source recovery does not invent discarded entered-text diffs.

## Read-only commands and exports

Foreground noninteractive commands use `ReadStore::open(&paths.database)` directly;
they need no terminal or worker. The connection is read-only and `query_only`.
It does not create directories, initialize SQL schema, migrate, change the journal,
or repair corrupted history. SQLite may need its normal existing-WAL coordination
sidecars; `immutable=1` is deliberately not used for a database that can change.

Resolve `--profile current` to the current effective `TestSpec::profile_key()` before
calling storage. `Filter::current(key)` selects that exact comparable profile;
`Filter::default()` explicitly selects all profiles. Optional fields are `mode`,
`language` (stable source ID), `outcome`, `from_utc_ms` inclusive, and `to_utc_ms`
exclusive. `parse_utc_bound(value, end)` accepts strict UTC dates or timestamps;
a date-only upper bound includes the entire named day. `Page` defaults to 20 rows;
limits 1–1000 and offsets are validated. `HistoryPage.next_offset` drives pagination.

```rust
use clack::storage::{ReadStore, Filter, Page, ExportFormat, export_records};
let history = ReadStore::open(&paths.database)?;
let page = history.history(&Filter::current(profile_key), Page::default())?;
let stats = history.stats(&Filter::default())?;
history.export_to(writer, ExportFormat::Jsonl, &Filter::default(), false)?;
// Recovery also works when the database is corrupt, full, locked, or unavailable.
export_records(writer, ExportFormat::Jsonl,
    store.pending().map(|(_, record, _)| record), false)?;
```

JSON is one versioned object with a results array. JSONL is one versioned record
per line. CSV includes the version, raw counts, exact elapsed microseconds, effective
spec, integrity, explicitly gated text fields, and text scope. Unavailable metrics remain JSON
`null` / empty CSV cells; nonfinite values are rejected. CSV quotes embedded
separators/newlines and prefixes formula-leading cells with an apostrophe for
spreadsheet safety; JSON preserves opted-in text exactly. Export holds one read
transaction and streams pages, so concurrent saves do not duplicate/skip rows.
No SQL or private source data is included in errors.

## Database, durability, and platform choices

Schema version 1 includes `results`, `samples`, `word_summaries`,
`schema_migrations`, `private_content`, `event_traces`, `profile_bests`, and
`profile_stats`. JSON export versioning is independent. Full effective specs,
app/scoring/generator versions, seed, raw counts, outcomes, reasons, assistance,
and integrity are retained. Header queries omit child arrays; a bounded 32-point
sparkline supports compact history. Detailed samples/words load only on demand.

An IMMEDIATE transaction inserts each result and its children, updates aggregate
counts, and compares the previous indexed best using `u128` cross-products of
credited units and elapsed time. Only a strict improvement changes the best; ties
preserve the earlier record. Seeds do not create categories; the engine's canonical
profile key owns effective-condition identity. Storage reuses the engine's single
eligibility predicate and also honors an explicitly revoked eligibility flag.
Paced, assisted, repeated, seeded, practice, and ineligible outcomes cannot update
the standard best even if a caller supplies a forged true flag. Weighted statistics sum counts and time, never rounded
run speeds. Target metrics exclude zen; output speed retains zen. Sample counts
and denominators are explicit, including zero-duration exclusions.

The profile/date index supports pagination, and materialized profile/outcome totals
support current-profile summaries without scanning all history. New databases are
created only by the worker. Unknown/newer schemas and corrupt databases produce
an explicit error and remain intact. Busy retries stay inside a 250 ms SQLite
budget. Every acknowledged transaction uses `synchronous=FULL`; macOS additionally
requests `fullfsync`. Durability still depends on the underlying filesystem/device.
[SQLite documents these synchronization semantics](https://www.sqlite.org/pragma.html#pragma_synchronous).

Automatic WAL selection positively recognizes macOS APFS/HFS and Linux ext4,
Btrfs, tmpfs, and overlay filesystems using safe `nix::statfs`. Other/unknown storage
uses DELETE journaling, including the unverified Windows automatic path. Explicit
`storage.journal_mode="delete"` and `--data-dir` provide the fallback/override.
Do not force WAL onto a network filesystem: SQLite's shared-memory WAL coordination
requires local storage. WAL is allowed only with the known WAL-reset correction
(bundled SQLite 3.51.3 or later, or the specified upstream backports).
[SQLite WAL documentation](https://www.sqlite.org/wal.html),
[Rusqlite 0.39.0 release](https://github.com/rusqlite/rusqlite/releases/tag/v0.39.0).

New Unix data directories use mode 0700 and database files 0600. Existing parent
directories are not chmodded. Parent paths are canonicalized because macOS's `/var`
is a normal symlink; the final database component remains protected by SQLite's
NOFOLLOW flag, avoiding both symlink replacement and chmod of unrelated targets.
On Windows, the storage worker creates missing data directories with protected
current-user DACLs and inheritable child-file permissions. New database files are
created with their private descriptor before SQLite writes any payload. Existing
shared directories or broad database/journal/WAL/SHM ACLs are refused without
changing them. Read-only opens validate present history too; missing history still
creates nothing. A directory-handle guard keeps the validated path in place until
after the SQLite connection closes. Unsafe/unsupported paths leave typing usable
and results explicitly unsaved, with guidance to choose a new data directory.
The safe application API is backed by the narrowly isolated Windows adapter;
native Windows ACL execution remains a separate validation requirement.
New helper-created objects explicitly belong to the current user. SQLite-created
children use the process's default owner: the adapter accepts the current user,
or Builtin Administrators only when that is the actual default owner, while still
requiring the sole effective full-control ACE to name the current user. Other
default owners are refused before SQLite opens. This does not attempt to exclude
administrator privileges, pre-authorized handles or same-user processes; the
reviewed trust boundary and native test requirements are recorded in
[the Windows permissions audit](audit-windows-permissions-review.md).

Checking each existing file is necessary because Windows does not use a parent's
DACL as the child's access check, and moving a file does not reset its descriptor.
[Microsoft file security documentation](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights)
describes these rules. Unix permission tests do not validate Windows ACL behavior.

## Coverage and validation

| Specification requirement | Implementation and evidence |
|---|---|
| STORE-001–005, REC-004,008 | Versioned SQLite schema, effective identity, child rows, indexes, read-only pagination/export; storage acceptance migration/filter tests. |
| STORE-006–009, ARCH-008,010 | Deferred one-worker API, immutable messages, bounded channels, FULL transaction/ack; root must verify first-frame wiring. |
| STORE-010–013 | Bounded pending set, returned ownership on capacity, retry/idempotency, bounded flush, recovery export; real lock/corruption tests plus SQLite FULL/read-only injection. |
| PRIV-001–003,005–007 | Central sanitization, independent opt-ins, source hash review, safe display validation, private Unix permissions and Windows guarded owner-only creation/refusal; explicit private/read-only tests, with native Windows ACL execution still pending. |
| REC-005–007,009 | Exact ratio comparison beyond f64 precision, outcome exclusions, asynchronous indexed queries, weighted sums and explicit sample counts. |
| REVIEW-003 | In-memory current review remains root-owned; historical original hash check never reconstructs discarded entered text. |
| PERF-010 | Explicit release benchmark seeds 100,000 rows and reports first-page/current-summary p95; no uncontrolled-machine absolute gate. |

Additional concurrency fixtures cover history created after read-only startup,
reconfiguration rejected under full worker/reply queues without partial policy
changes, and a 201-row paginated export while another WAL writer commits. The
export must retain its original consistent snapshot and expose the new record
only on a later query.

Run `cargo test --test storage_acceptance` and `cargo test --lib storage::` for
functional checks. Run the ignored large-dataset measurement explicitly:

```sh
cargo test --release --test storage_acceptance \
  history_100000_rows_reports_first_page_and_summary_p95 -- --ignored --nocapture
```

The local release measurement on the Apple M3 / macOS Darwin 25.5.0 host recorded
first-page p95 **177 µs** and current-profile-summary p95 **14 µs**, using 100
measurements after five excluded warmups over 100,000 seeded matching results.
The fixture explicitly used DELETE journaling and bundled SQLite 3.51.3. It measured
SQL queries with no terminal involved. Exact command, compiler profile, binary and
source hashes, hardware, dataset, and limitations are retained in
`docs/measurements/storage-history-100k.json`; the release test executable is
preserved under `target/benchmarks/storage/`. These observations are below the initial 100 ms target, but
the shared development host is not a controlled regression reference. The run did
not retain raw sample arrays and does not measure bulk save throughput.

Actual storage throughput/failure behavior on unavailable operating systems,
network mounts, full physical volumes, power loss, and device sync failures remain
external validation. SQLite's deterministic `max_page_count` FULL injection tests
the production transaction path; it is not a claim that a physical disk was filled.

The later frozen-core measurement is
`docs/measurements/stage-d-storage-history-100k.json`: first-page p95 **167 µs** and
summary p95 **12 µs**, again 100 measured warm queries after five warmups. The
single measured run, successful output, exact release-test hash, source manifest,
and report-wrapper corrections are preserved. This measurement preceded the
Windows-only ACL integration and is not silently relabeled as a newer build.

The final runtime freeze `31586c3b…` has a separate release measurement in
`docs/measurements/stage-d-storage-history-100k-permissions-final.json`: first-page
p95 **170 µs**, summary p95 **14 µs**, from one invocation with the same five warmups
and 100 measured queries. Its manifest contains exact final source inputs, test
source and executable hashes. All project builds and terminal/package workloads
were paused; the earlier long Zen process had completed. These remain warm-query
observations on a shared development host, with no raw sample-array or native
Windows runtime claim.

After the Windows storage branches were added, all 5 storage unit tests and 29
acceptance tests passed again on macOS under Rust 1.95 and Rust 1.88, with all
features. `docs/measurements/stage-d-storage-permissions-unix-checks.log` records
those checks. The two new Windows-only integration fixtures cover private WAL/SHM
inheritance, parent-handle lifetime, unsafe-parent refusal, unchanged existing
files and unsaved snapshot retention; they require native Windows execution and
are not counted as passed by the macOS runs.
