# Performance measurement methodology

The specification's numbers are acceptance targets. A Rust build, a microbenchmark,
or a PTY script does not establish real terminal responsiveness. Keep the raw report,
the binary hash, the machine description, and the exact workload together. Do not
replace an unperformed measurement with an estimate presented as a result.

## Tools and scope

`scripts/benchmark.py` launches the production executable through the shared
`PtyProcess` in `scripts/pty_test.py`. The helper gives the child a controlling
terminal, independent source stdin/stdout/stderr channels, geometry, bounded output
capture, and a bounded in-memory VT screen. The benchmark imports it with
`observe=False`; no test-hook notifications participate in performance measurements.

The script uses Python's standard library. It makes no network calls, installs no
packages, purges no filesystem caches, and reads no existing user history/config.
Each workload has an isolated temporary config and data directory. Default runs use
`--private` to suppress persistence; `--with-history` enables ordinary saves in the
isolated database. Reports label this difference. Real references should include a
second run with normal storage enabled.

The benchmark source itself does not collect terminal input text. Application
observations are finite numeric JSON written once **after terminal restoration** to
the explicitly requested `--benchmark-output` path. Raw display bytes are discarded
after each read; the helper maintains only the current screen and bounded captures.
Reports describe synthetic text by byte count/hash rather than storing its body.

Supported automated host accounting is Linux `/proc/PID/stat` and macOS `ps`.
The script records unavailable accounting as unavailable, never as zero CPU. Windows
needs a separate native pseudoconsole runner and actual Windows Terminal testing;
Unix PTY results are not Windows evidence.

## Build and run

Build and measure the ordinary optimized executable with locked dependencies:

```sh
cargo build --release --locked
python3 scripts/pty_test.py --self-test
python3 scripts/benchmark.py --self-test
python3 scripts/benchmark.py --binary target/release/clack --suite quick \
  --build-description "cargo build --release --locked; default features" \
  --output target/benchmarks/quick.json
```

The quick suite is a harness/initial-latency check. It uses ten measured warm
launches and 25 ASCII plus 25 mixed-width latency deliveries at 120×40. It does
not replace the 60-second idle intervals, 1,000-run reset experiment, sustained
stress, all geometries, or actual emulator checks.

The complete ordinary PTY suite is:

```sh
python3 scripts/benchmark.py --binary target/release/clack --suite full \
  --startup-runs 100 --latency-events 400 \
  --build-description "cargo build --release --locked; default features" \
  --output target/benchmarks/full.json
```

Use `--controlled-reference NAME` only for an established dedicated reference
machine with documented power/performance settings and a controlled background
load. Add `--portable-cpu-target-confirmed` only after checking the actual compiler
flags and Cargo configuration. It is a recorded operator confirmation, not a claim
the harness recovered code-generation flags from the executable. Do not use
`target-cpu=native` for distributed binaries. Record any LTO/codegen-unit changes
and compare the resulting executable size and actual performance.

The application emits `debug_assertions` and `test_hooks_enabled` as numeric build
flags when available. Only two zero values establish the script's
`production_build_verified` field. Missing flags leave the build unverified.
Test-hook builds are useful for correctness/lifecycle tests and must not be used to
claim production latency or startup acceptance.

## Measurement definitions

### External startup

`started_ns` is the shared helper's monotonic timestamp immediately before creating
the actual application process. On a host requiring a separate terminal-session
keeper, that keeper's own interpreter startup is excluded; clack's process creation,
executable loading, config/content preparation, and terminal initialization remain
included. The endpoint is the first observed current VT screen with the complete
Ready hint, a visible caret, and the default typing surface. No input is sent to
start or focus the test.

The endpoint is an **external receipt upper bound**, not an internal `main` timer.
It includes OS scheduling, PTY transport, and the Python reader/screen-parser cost.
Reads wait on readiness with a maximum 5 ms timeout; that timeout does not add a
fixed 5 ms sleep when bytes are ready. Keep observer effects in mind around a tight
50 ms p95 threshold. A timeout or exited process is a failed experiment, not a
startup sample. The application may report its own `first_usable_frame_us`, but
that internal interval must not be substituted for total startup.

Warm runs exclude the first three launches by default. The executable hash is read
and the environment collected before ordinary warm experiments, so the script
explicitly treats those runs as warm-cache measurements. The report retains every
measured startup duration and percentile sample count.

For a separately prepared cold experiment:

```sh
python3 scripts/benchmark.py --binary target/release/clack --suite cold \
  --cold-methodology "Describe the actual reboot/cache preparation and first-launch procedure" \
  --output target/benchmarks/cold.json
```

Cold mode does not read/hash/run the executable before its first launch. It collects
environment and binary metadata afterward. It does not verify that the kernel,
filesystem, storage controller, or device caches were cold. The report therefore
retains `cold_cache_state_verified=false` and the operator's exact procedure.
Attach independently established evidence before describing a launch as genuinely
cold. One declared-cold run is one observation, not a statistically reliable p95.
No cold result has been measured merely by providing this command.

### Input latency

There are two distinct latency metrics:

* `received_to_completed_flush_us` uses the application's input-reader receipt
  timestamps and completed coherent-frame flush timestamps. It is the metric for
  the p95 ≤10 ms / p99 ≤20 ms application acceptance targets. Input updates must not
  be deferred to satisfy the render ceiling. Bounded observations also expose total
  samples and dropped samples; the report preserves those counts.
* `pty_delivery_to_next_output_us` starts immediately before the harness writes a
  text unit into the PTY and ends when subsequent PTY output is received. It includes
  input transport and observer scheduling. A timer frame can be the next output,
  so it is supplementary evidence and must not stand in for the first metric.

Latency probes are separated by 25 ms of observation time. A probe with no output
within 250 ms increments `output_timeouts`; it is not converted into a fast sample.
The default ASCII trace is English 200, generator version 1, seed 42, numbers and
punctuation disabled. Its independently generated prefix is checked against the
committed golden vector. The mixed-width exact fixture contains composed and
decomposed accents, double-width CJK characters, and an emoji joiner sequence.
These UTF-8 deliveries are logical scripted units, not physical keyboard presses
or composition-aware IME measurements.

The full suite runs both traces at 80×24, 120×40, and 200×60. The 120×40 default
ASCII run is the primary application-latency comparison. Report enabled live
metrics/pacing/color separately if those settings are changed from the defaults.

### CPU and memory

CPU percentages mean accumulated user+system CPU divided by elapsed wall time,
as a percentage of **one logical core**. Linux uses the kernel's process tick
counts; macOS uses `ps`'s accumulated process time. Reports include the accounting
resolution and its percentage of the measurement interval. A rounded zero is not
proof that the application consumed no CPU.

Ready and Results each have a separate 60-second idle observation. A one-second
timed run enters Results first, followed by a 250 ms settling interval. The report
also records bytes emitted during idle. Shorter `--idle-seconds` settings are
permitted for diagnosis but set `required_gate_interval_met=false`.

Active CPU uses a correct deterministic ASCII trace delivered at 12.5 logical
characters/second, the five-character-equivalent rate of 150 WPM. Scheduling uses
absolute monotonic deadlines, reports actual delivered rate and maximum scheduling
lag, and includes an endpoint frame. It records the exact duration/profile flags.
The controlled reference target is <3% of one logical core at 120×40.

The 1,000-run memory experiment completes normal one-word tests using a nonempty
wrong word and allowed final Space submission, then immediately sends Ctrl-R and
waits for the next Ready screen. It samples RSS through the run sequence and reports
the first-to-last change in the latter half. This tests repeated completion/reset
paths; it is not mislabeled as 1,000 complete 30-second tests. Repeatable continued
growth after allocator warmup needs investigation. A single RSS plateau does not
prove every allocation is leak-free. Default RSS has a ≤30 MiB target.

### Stress, slow consumers, and long zen

Healthy and slow-consumer stress each request 1,000 ASCII text events/second for
five seconds at every required geometry. The slow case reads the PTY at 250 ms
intervals, producing bursty output consumption and possible backpressure. It is
an interval-throttled consumer, not a model of a specific terminal or SSH link.
The shared writer can drain output if needed to make progress on a blocked PTY
write; actual send rate and scheduling lag remain part of the report.
The report separately labels the requested 1,000-event/second workload and whether
delivery came within one percent of its requested rate. A throttled generator that
delivered materially fewer events per second has not exercised the required load.

Stress success is ordered application input with matching delivered/applied counts,
or a reported explicit overload interruption. The harness needs numeric
`text_events_applied_count`, `sequence_violation_count`, and `input_overload_count`
to establish that condition. Missing hooks produce an explicit **unverified**
assertion. Dropped observations are different from dropped input. Report overloads
as interruptions; do not turn a 1,000-event/second stream into a WPM claim.

The default zen experiment sends 20,480 units, cycling the 4,096-unit editable text
window more than four times. It measures RSS after saturation and records actual
buffer high-water counts. Cumulative scored `retained_units` can grow after text is
aggregated; the relevant memory bound is editable-buffer length.
Observed editable lengths above 4,096 or chart lengths above 3,601 fail the
experiment. Missing buffer hooks stay explicitly unverified. The final delivered
and applied text counts also show whether the requested window cycling reached the
application.

The chart holds 3,601 one-second samples. The default accelerated input stream does
not cycle that real-time window. To exercise it in the running application:

```sh
python3 scripts/benchmark.py --binary target/release/clack --suite zen \
  --zen-units 20480 --zen-seconds 3610 \
  --output target/benchmarks/zen-long.json
```

The script records RSS every 30 seconds after the text stream finishes and explicitly
reports whether the chart-saturation interval was reached. Pure-engine injected-time
tests can separately verify sample-window bounds without sleeping; their evidence
does not substitute for this application-level memory observation.

## Reports, regression comparisons, and external gates

The initial pre-storage Stage B baseline ran on an Apple M3 (8 logical cores,
16 GiB RAM), local macOS/Darwin 25.5.0, Rust 1.95.0 release with default features.
The preserved executable SHA256 is
`b5400990c91074e5529fe8dcaa4782253a2ea1e5a89d4e6dead5cc545d8bfe5d`
(2,560,688 bytes). These are Unix PTY observations with inherited `NO_COLOR=1`,
not actual-emulator measurements, verified cold launches, controlled-reference
results, or validation of the final application after storage integration.

| Observed baseline | Actual result |
|---|---|
| Warm startup, 50 launches per geometry | p95 4.789 ms at 80×24, 5.707 ms at 120×40, 4.866 ms at 200×60 |
| ASCII receive-to-flush, 200 probes at 120×40 | p95 0.745 ms, p99 10.167 ms |
| Mixed-width receive-to-flush, 200 logical deliveries at 120×40 | 249 decoded scalar observations; p95 0.540 ms, p99 0.638 ms |
| 150 WPM replay, 30 seconds at 120×40 | 0.4664% of one core |
| Ready and Results idle, 60 seconds each | CPU rounded to 0 at 10 ms accounting resolution; zero application output bytes |
| 1,000 completed one-word reset cycles | All 1,000 confirmed; sampled RSS max 4,603,904 bytes; latter-half change 98,304 bytes |
| Healthy and slow-consumer stress at all three geometries | All 5,000 events applied in order per case at approximately 1,000/s; no overload; slow consumer caused expected backpressure latency |
| Real-time Zen, 3,610.0139 seconds at 120×40 | All 20,480 text events applied; editable high-water 4,096; chart high-water 3,601; no order/overload/buffer violations |
| Zen memory over that hour | RSS max 5,439,488 bytes, final 4,358,144 bytes; post-text-saturation first-to-last change −983,040 bytes |

Every measured process restored its terminal and left reserved stdout/stderr
empty. The long Zen run contains only about 10 seconds after chart saturation;
it verifies that the running application's chart window filled and cycled, but
does not establish a lengthy post-chart RSS plateau. Production observations
confirmed `debug_assertions=false` and `test_hooks_enabled=false`.

Raw reports are archived as `docs/measurements/stage-b-quick.json`,
`stage-b-full.json`, and `stage-b-zen-long.json`. The measurement manifest records
their sizes, SHA-256 digests, schema versions, original paths, and phase limits.
Both historical 100,000-row storage reports are archived beside them. The
preserved baseline executable is `target/benchmarks/stage-b/clack`.
Source/compiler/storage settings changed afterward. Remeasure the final release,
including ordinary persistence, before using these observations as release evidence.

Reports use versioned JSON with finite numbers/null, nearest-rank percentiles,
sample counts, exact OS/CPU/RAM/architecture, installed Rust/compiler version,
binary SHA-256/size, build description, local/SSH execution, TERM/color settings,
geometries, workload flags, private/storage mode, and an initially empty isolated
history. Actual emulator name/version is **null**, because this harness does not
launch an emulator. Its current VT parser is a diagnostic observer only.

The output exit status is nonzero for experiment errors, lost-input assertions,
latency-probe timeouts, or failed terminal restoration. Absolute timings do not
turn into a misleading shared-CI pass/fail score. For a recorded reference:

```sh
python3 scripts/benchmark.py --suite full --controlled-reference machine-name \
  --baseline /path/to/previous-report.json \
  --output target/benchmarks/comparison.json
```

Comparison requires matching named machine, OS, architecture, core count, storage
mode, and geometry. A ≥10% p95 increase asks for repetition and investigation; one
run is not enough to call a regression repeatable. Different power modes, thermal
conditions, terminals, profiles, histories, options, or observer versions need
separate baselines even when their reported CPU names match.

The following remain separate measurements: warm reducer p99/no-allocation gates,
in-memory renderer CPU, 100,000-row history query p95, dependency/license review,
network-traffic observation, actual local terminal input-to-flush behavior, and
physical key-to-photon latency. The terminal matrix must include actual macOS
Terminal, Windows Terminal, a Linux emulator, Kitty protocol and baseline input,
tmux, and SSH. These documents and PTY runs do not establish unsupported hardware,
operating-system, emulator, IME, or screen-reader results.

## Storage-enabled final-run checks

With `--with-history`, every session reads its isolated SQLite database after the
application exits. The report compares stored rows with the numeric count of
finalized complete/failed/incomplete/aborted/interrupted runs and reports the
maximum saved sample count. This query occurs after the measured latency/CPU
interval and cannot contaminate the active measurements. The checker also verifies
that default records contain no private bodies, event traces, or retained entered
strings. Private runs require no database creation at all. A missing observation
file, a nonproduction build, unexpected stdout/stderr, storage count mismatch, or
privacy mismatch marks the workload failed instead of silently retaining its
timing as a passing result. Earlier Stage B reports predate these SQL checks and
remain explicitly labeled as the pre-storage baseline.

## Packaged ARM64 measurements before the Windows permission correction

The storage-enabled full suite before the last Windows-only correction is archived in
`docs/measurements/stage-d-full.json`. All 22 workload groups completed with zero
harness failures on the Apple M3 host (8 logical cores, 16 GiB RAM), Darwin 25.6.0,
Rust 1.95.0, local Unix PTYs, `TERM=xterm-256color` and inherited `NO_COLOR=1`.
This host's OS differs from the older Stage B run; the reports are not a controlled
regression comparison. Project builds and other measured workloads were paused;
the independent long Zen process remained active, and unrelated host activity was
not controlled. No dedicated-reference or actual-emulator label is claimed.

The exact packaged ARM64 snapshot is 5,047,968 bytes, SHA256
`1f9c13ef3283460d586cca173d9ef6e6873d4db764271c95591f538219ced590`.
Its attached manifest records the explicit `aarch64-apple-darwin` target, source
identity `a3c58706a5ba1de96232ad4d49ab7f734ffea6d4c8dfbfd26bab2b8cd71a1545`,
optimization level 3, thin LTO, one codegen unit, `strip=symbols`, unwind panics,
no test hooks, empty encoded Rust flags and macOS deployment target 11.0.
The source/build identity is embedded in the report, rather than inferred from a
subsequently rebuilt `target/release/clack`.

| Observed workload on the identified earlier snapshot | Actual result |
|---|---|
| Warm startup, 50 measured launches after three warmups at each geometry | p95 4.540 ms at 80×24, 4.729 ms at 120×40, 4.769 ms at 200×60 |
| Ordinary ASCII receive-to-flush, 200 probes at 120×40 | p95 0.660 ms, p99 1.400 ms |
| Mixed-width receive-to-flush, 200 logical deliveries at 120×40 | 249 scalar observations; p95 0.585 ms, p99 0.651 ms |
| 150 WPM replay for 30 seconds at 120×40 | 0.7995% of one logical core; actual pacing 12.4994 logical units/s |
| Ready and Results idle for 60 seconds each | CPU below the 0.01-second accounting resolution; zero output bytes and unchanged RSS |
| 1,000 completed one-word tests and restarts | Exactly 1,000 finalized rows persisted; sampled RSS max 9,732,096 bytes; latter-half increase 49,152 bytes, then flat from recorded run 661 through run 1,000 |
| Six healthy/250-ms-throttled stress workloads across all geometries | 5,000 ordered events per case at at least 999.927 events/s; delivered/applied counts match, no overload or sequence violations |
| Short packaged-binary Zen text-window run | All 20,480 text events applied; editable high-water 4,096; terminal restored; one result persisted |
| Earlier-source 100,000-row query fixture | 100 measured warm queries after five warmups: first-page p95 167 µs, current-profile summary p95 12 µs |

The rapid one-word restart workload has receive-to-flush p95 10.184 ms and p99
11.023 ms. Its p95 is slightly above the ordinary-input 10 ms target; it is not
hidden behind the faster default timed-text probe. The intentionally throttled
120×40 stress consumer has p95 190.667 ms and p99 199.390 ms from backpressure;
stress acceptance is ordered input or explicit overload, not ordinary interactive
latency. All default privacy checks found zero private bodies, event traces and
retained entered-text rows. Every measured process restored terminal attributes
and left reserved stdout/stderr empty.

The query result and unchanged successful output are
`docs/measurements/stage-d-storage-history-100k.json` and `.log`. Its separate
release test executable links the verified frozen production source and is tied
to `stage-d-storage-build-manifest-v2.json`; the selected fixture explicitly uses
DELETE journal mode. The report records its one measured attempt and two wrapper
corrections. No query measurement was rerun to select a faster result. The existing
fixture emits exact p95 values rather than raw arrays, which the report discloses.

## Actual long Zen with integrated storage

The uninterrupted native long Zen run completed after **3,610.013833 seconds**.
`docs/measurements/stage-d-zen-long.json` records all 20,480 delivered units applied,
zero overload/order/bound failures, an editable high-water mark of 4,096 and a
chart high-water mark of 3,601. F5 completed the run; one acknowledged result with
3,601 samples was found after exit, with zero default private bodies, traces or
entered strings. The subsequent harness Ctrl-C closed Results with status 130;
the saved run is complete, not interrupted. Terminal attributes were restored and
reserved stdout/stderr remained empty.

This uses a separate native-target snapshot, 5,047,952 bytes, SHA256
`364c3b402cb9bd6a2c4d858ff28af881fc328ac7d9557ec52d343d93c3ae32b2`.
It is not silently identified as the 16-byte-larger explicit-target package binary.
Its source remains `a3c58706…`, preceding the Windows-only correction.
`stage-d-measurement-source-relationship.json` verifies that engine, content,
rendering and terminal inputs are byte-identical in the final `31586c3b…` freeze.
The launch's freeform description mistakenly named fat LTO/stripped debuginfo;
its captured build manifest correctly records thin LTO/stripped symbols. The
corrected report retains the original value, original-report hash and unchanged
measurement hash. `stage-d-zen-long-original-description.json` preserves the exact
unmodified original output. The measurements have not been relabeled or edited.

The input burst takes 20.479 seconds; the application then accumulates real chart
time until the full 3,610-second interval. This is not an hour of continuous typing.
There are 140 RSS observations, at most 7,340,032 bytes and ending at 3,719,168 bytes.
Only one RSS observation is after the chart fills, so this run does not establish
an extended post-chart-saturation RSS plateau. The harness's `post_saturation`
fields refer to the editable text window, not the hour-long chart. Lower later
RSS on the shared host is not attributed to engine deallocation. Cross-compilers,
profile-comparison builds and the later Windows correction overlap portions of
this bounds run; no quiet/reference-machine timing claim is made.

`docs/measurements/zen-bounds-final.json` separately records a new independent
engine oracle over 26,207 injected full seconds plus 375 ms, six repeated chart
cycles, exact cumulative history and correction limits, zero warmed-cycle
allocations, and zero live allocations after engine/snapshot destruction. Both
current Rust and the MSRV pass with their recorded commands. This strengthens
state-lifetime evidence but does not replace real elapsed application/terminal/RSS
measurements.

## Final-source history query measurement

`docs/measurements/stage-d-storage-history-100k-permissions-final.json` records a
new single release-test invocation after the final `31586c3b…` freeze. First-page
p95 is **170 µs** and current-profile summary p95 is **14 µs**, with five warmups
and 100 measured queries over 100,000 seeded rows. The new build manifest records
all source inputs, exact test source and executable SHA; the original 167/12 µs
measurement remains preserved. Project compilers, terminal suites and package
work were paused, and the long Zen process had completed. This remains a shared
development host and a warm query measurement, not a controlled regression gate.

## Exact final packaged full suite

`docs/measurements/stage-d-full-permissions-final.json` records all **22** workload
groups measured with zero harness failures on the final packaged ARM64 executable,
SHA256 `4e50a20121623d2634fbdbda9aa1bdf8c066cc2e6623f0b071c1e9d9125c5807`,
5,047,968 bytes, source `31586c3b…`. Its embedded manifest records Rust 1.95,
opt3, thin LTO, one codegen unit, stripped symbols, portable CPU settings and macOS
11.0 deployment. Host, PTY, default config and privacy settings match the stated
Darwin 25.6.0 development environment. All project compilers, terminal suites and
package/install work were paused, and the long Zen process had completed. Small
documentation writes and uncontrolled unrelated host activity remained. This is
a new complete measurement because the final executable hash changed; the earlier
report is preserved rather than relabeled or selectively replaced.

| Geometry | Warm startup p95 | ASCII receipt→flush p95 / p99 | Mixed receipt→flush p95 / p99 | 150 WPM CPU, one core |
|---|---:|---:|---:|---:|
| 80×24 | 4.919 ms | 0.774 / 2.021 ms | 0.695 / 1.527 ms | 0.5663% |
| 120×40 | 5.068 ms | 0.816 / 1.945 ms | 0.866 / 1.547 ms | 0.6663% |
| 200×60 | 5.190 ms | 1.507 / 2.641 ms | 1.406 / 1.923 ms | 0.9661% |

Startup again includes 50 measured launches after three warmups per geometry.
The ordinary probes retain 200 ASCII or 249 mixed scalar observations per geometry.
Both real 60-second idle intervals emit zero bytes and remain below the 0.01-second
CPU-accounting resolution. All 1,000 successive one-word normal completions are
saved; sampled RSS peaks at 9,781,248 bytes, with a 32,768-byte latter-half change
and unchanged observations from run 551 through run 1,000. All checked databases
match finalized counts and preserve default privacy. All measured processes
restore terminal attributes and leave reserved stdout/stderr empty.

The rapid restart workload records p95 **10.634 ms** and p99 **11.671 ms**, again
slightly above the ordinary-input 10 ms p95 target; no blanket all-workload latency
pass is claimed. Each of the six stress cases applies all 5,000 delivered events
in order, at actual rates of at least 999.917 events/s, with no overload. The
250-ms-throttled 120×40 consumer records p95/p99 189.130/198.961 ms, reflecting
intentional output backpressure. The short Zen case applies all 20,480 units,
retains at most 4,096 editable units and 21 chart samples over 20.493 seconds,
and saves one result. It does not substitute for the separately identified real
hour-long or repeated-cycle engine experiments.
