# Release performance report

These are observed development-host results, with separate executable identities
for every experiment. They are not dedicated-reference results, genuine cold
launches, actual graphical-terminal measurements, or physical key-to-photon timing.
No unexecuted gate is a pass. The complete final-package performance suite and
final history query measurement use build-input identity
`31586c3bc81633d5d3f912b7e717800caf185ecfce5358f7feeb92b3aa8129e3`.
The separate profile comparison and hour-long Zen experiment retain original
identity `a3c58706a5ba1de96232ad4d49ab7f734ffea6d4c8dfbfd26bab2b8cd71a1545`.
Their unchanged-engine relationship is documented below; no report is relabeled
with another executable hash.

## Machine and build

The final full suite ran September 6, 2026 local time (September 7 UTC) on an
Apple M3, eight logical cores, 16 GiB RAM, macOS 26.6.2 build 25G83, Darwin 25.6.0.
Rust was 1.95.0 (`59807616e`, LLVM 22.1.2); the minimum supported Rust is 1.88.0.
The release profile was opt-level 3, thin LTO, one codegen unit, stripped symbols,
unwind panic handling, and no debug assertions or test hooks. Distributed builds
use portable CPU settings. macOS deployment is 11.0.

The full suite's exact native ARM64 production executable is 5,047,968 bytes,
SHA-256 `4e50a20121623d2634fbdbda9aa1bdf8c066cc2e6623f0b071c1e9d9125c5807`.
Its source identity, original command, compiler flags, and executable SHA are
embedded in [the raw full report](measurements/stage-d-full-permissions-final.json). Each process
uses isolated schema-version-1 defaults and a new initially empty local SQLite
history, with explicit workload options recorded. Saved bodies, traces and entered
text remain disabled. No existing user configuration or history is read.

The terminal is a local Unix PTY, observed by the bounded Python 3.12.5 VT parser;
there is no actual emulator name or version to report. The inherited environment
is `TERM=xterm-256color`, `COLORTERM=truecolor`, `NO_COLOR=1`, `LANG=LC_ALL=C.UTF-8`.
Thus these runs use the monochrome policy. Project builds, terminal suites,
package/install work and competing measured workloads were paused during timing.
The long Zen process had finished. Small documentation writes and unrelated host
activity remained; power/thermal/background conditions were not controlled.

## Startup, ordinary latency and active CPU

All 22 workloads completed with zero harness failures. Startup is measured
externally from the actual application fork to the complete usable Ready screen,
including content preparation and terminal initialization. Each geometry has
50 measured warm launches after three excluded warmups. The helper's session
keeper preparation is excluded; no timestamp inside `main` replaces process start.

| Geometry | Warm startup p95 | ASCII receipt→flush p95 / p99 | Mixed receipt→flush p95 / p99 | 150 WPM CPU, one core |
|---|---:|---:|---:|---:|
| 80×24 | 4.919 ms | 0.774 / 2.021 ms | 0.695 / 1.527 ms | 0.5663% |
| 120×40 | 5.068 ms | 0.816 / 1.945 ms | 0.866 / 1.547 ms | 0.6663% |
| 200×60 | 5.190 ms | 1.507 / 2.641 ms | 1.406 / 1.923 ms | 0.9661% |

The ordinary ASCII probes use English 200, generator version 1, seed 42 and no
modifiers. There are 200 probe deliveries per geometry. The 200 mixed-width
deliveries produce 249 reader scalar observations, including combining marks,
wide CJK and a joiner sequence. The primary default 120×40 observations meet the
specified 50 ms startup, 10/20 ms application latency and 3% CPU targets on this
host. The observations remain distinct from the required controlled and emulator
gates. Application receipt→completed coherent flush is not physical keyboard or
display latency. The report also preserves the supplementary external
PTY-delivery→next-output observations, which can include timer output.

CPU uses actual accumulated application user+system time over approximately
30.02 seconds, with 375 units delivered at approximately 12.4998 units/second at
120×40. macOS accounting resolution is 0.01 seconds, about 0.0333 percentage
points for this interval. Input/output/setup work outside that interval is not
silently included as measured active typing CPU.

## Idle, persistence, repeated tests and stress

Ready and Results each have a separate real 60-second observation after settling.
Both emit zero bytes during the idle interval and have CPU deltas below the
0.01-second accounting resolution: less than approximately 0.0167% of one core,
not a claim of literally zero CPU. Ready RSS is 7,979,008 bytes and Results RSS is
8,388,608 bytes. Active default 120×40 RSS ends at 10,321,920 bytes, below 30 MiB.

All 1,000 successive one-word normal completions were acknowledged in SQLite;
the post-exit query finds exactly 1,000 records. This is a repeated completion/reset
test, not 1,000 full 30-second sessions. Across 101 RSS observations, the maximum
is 9,781,248 bytes. The latter-half change is 32,768 bytes; the complete sampled
trajectory remains in the raw report. Default private text, entered strings and
diagnostic traces are absent in every checked database. Finalized counts and
stored rows agree in all workloads; every process restores its terminal and
leaves reserved stdout/stderr empty.

| Geometry | Healthy: actual events/s; p95 / p99 | Slow consumer: actual events/s; p95 / p99 |
|---|---:|---:|
| 80×24 | 999.967; 8.766 / 9.149 ms | 999.955; 190.691 / 198.034 ms |
| 120×40 | 999.917; 8.959 / 10.025 ms | 999.956; 189.130 / 198.961 ms |
| 200×60 | 999.963; 9.350 / 10.080 ms | 999.951; 191.459 / 204.670 ms |

Each stress case delivers and applies all 5,000 events in order with no overload
or sequence violation. The actual rate is within 0.01% of the requested
1,000 events/second. Slow consumers drain output every 250 ms; their latency is
intentional backpressure and is not presented as ordinary typing latency or a
WPM achievement. The rapid one-word restart workload also has a 10.634 ms p95
receipt→flush interval (p99 11.671 ms), slightly above the default trace's 10 ms
target. It is reported separately instead of claiming every workload meets that
ordinary-input target. Numeric flush counts and exact output-byte totals for
every workload are retained in the raw report.

## Reducer, render CPU and profile comparison

This comparison predates the Windows permissions correction and retains its
original build-input identity `a3c58706…1545`. The engine and renderer inputs are
byte-identical in the final source; these are observations of the stated earlier
executables, not new measurements of the final package.

The deterministic reducer benchmark applies 50,000 ASCII actions, including
incorrect attempts, corrections and submissions. Both compared profiles produce
40,001 correct attempts out of 45,001 attempts, 5,000 deletions, and 40,001 retained
and credited units. Both perform zero measured heap allocations and allocate zero
bytes during the warmed reducer trace.

| Measured build choice | Main executable bytes | Reducer p50 / p95 / p99 | Render child user+system CPU |
|---|---:|---:|---:|
| opt3, thin LTO, 1 codegen unit | 5,047,952 | 125 / 958 / 1,542 ns | 3.980706 s |
| opt3, no LTO, 16 codegen units | 5,964,208 | 125 / 958 / 1,750 ns | 5.108131 s |

The native compute build omits an explicit `--target` argument and differs by
16 bytes from the earlier packaged ARM64 binary. Those earlier builds have the
same audited build-input identity and relevant profile; their individual
immutable manifests preserve the distinction. This compares two complete profile choices; it does not isolate
LTO's effect from the codegen-unit change. The smaller and faster observed
thin/codegen1 choice is retained; opt-level `z` was not assumed faster.

Rendering uses Ratatui's in-memory TestBackend, a complete frame, buffer diff and
in-memory flush. Per-frame timings exclude engine application and preparation.
Each geometry/trace has one excluded warmup and 30 measured runs of 375 text
events plus Ready/Results, totaling 11,310 frames. There is exactly one layout
rebuild per run, confirming that ordinary correct input reuses prepared layout.

| Geometry / trace | Thin/codegen1 frame p95 / p99 | No-LTO/codegen16 frame p95 / p99 |
|---|---:|---:|
| 80×24 ASCII | 31.166 / 39.708 µs | 36.500 / 47.166 µs |
| 80×24 mixed | 24.667 / 27.750 µs | 31.375 / 35.667 µs |
| 120×40 ASCII | 43.917 / 48.500 µs | 59.750 / 62.542 µs |
| 120×40 mixed | 46.458 / 51.000 µs | 62.416 / 65.667 µs |
| 200×60 ASCII | 100.000 / 107.583 µs | 129.250 / 136.042 µs |
| 200×60 mixed | 109.583 / 114.042 µs | 132.625 / 138.625 µs |

The raw [thin profile](measurements/stage-d-compute-thin.json) and
[no-LTO profile](measurements/stage-d-compute-no-lto.json) preserve compiler,
trace, frame counts, process wall/CPU times, executable hashes and sizes.
These timings establish neither PTY latency nor actual-emulator behavior.

## History queries and bounded long sessions

The [final-source 100,000-row report](measurements/stage-d-storage-history-100k-permissions-final.json)
and its unchanged raw log record a release storage executable, SQLite 3.51.3,
100,000 seeded same-profile rows, five warmups and 100 measured warm queries.
The first 20-row current-profile page has p95 170 µs; the weighted current-profile
summary has p95 14 µs, each below the 100 ms target. This measures indexed reads,
not save throughput. The exact source/test/build identities and successful
summary output are preserved; this measured invocation ran once. Individual
timing arrays were not emitted or retained. The earlier 167/12 µs observation
remains in its original report under its earlier source/executable identity.

The final full application's short Zen case applies all 20,480 units at 999.987 events/s,
cycles the 4,096-unit editable window five times, and observes 21 chart samples
over 20.493 seconds. It does not cycle the hour-long chart window.

The independent [Zen lifetime fixture](measurements/zen-bounds-final.json) passes
on Rust 1.95 and 1.88. It injects 26,207 seconds plus a 375 ms tail, compares each
retained sample and literal grapheme against a complete-transcript oracle,
exercises six excess-deletion/drain/refill cycles, and includes idle intervals,
Unicode and correction. After both windows saturate, six additional 3,701-second
cycles produce zero allocations and zero added live or peak engine bytes. The
warmed live engine allocation is 1,211,594 bytes; dropping the engine and final
snapshot releases all measured allocations. There are 118,460 aggregate units
outside editable memory. This is deterministic engine/lifetime evidence, separate
from application RSS and real elapsed time.

The actual [production Zen experiment](measurements/stage-d-zen-long.json)
completes **3,610.014 real seconds**. It applies all 20,480 units in an initial
20.479-second burst, then accumulates chart time until F5 completes the run.
The editable cap is 4,096 units; the chart cap and saved sample count are 3,601.
Exactly one result is finalized and saved, with zero overload, ordering or
default-privacy failures. The terminal is restored and stdout/stderr remain
empty. Ctrl-C exits Results afterward, so exit 130 is intentional cleanup and
does not denote an interrupted stored result.

The 140 RSS observations remain in the raw report. Only one falls after the
chart window saturates, so the run does not establish an extended post-chart
RSS plateau. The initial burst followed by chart accumulation is not described
as an hour of continuous typing. Concurrent build/host activity during this
bounds experiment is disclosed; falling RSS is not attributed to deallocation.
The independent six-cycle allocation oracle supplies the repeated saturation
evidence.

This run retains original native executable SHA-256
`364c3b402cb9bd6a2c4d858ff28af881fc328ac7d9557ec52d343d93c3ae32b2`.
The [source relationship](measurements/stage-d-measurement-source-relationship.json)
verifies that engine, content, rendering and terminal inputs are unchanged by
the later Windows-only integration. A launch-description typo said fat LTO and
stripped debuginfo; the launch-time manifest correctly recorded thin LTO and
stripped symbols. The annotated report corrects only that prose and preserves
the original unmodified report and identical measurements.

## Network, older evidence and remaining gates

Static application and dependency review finds no runtime networking, telemetry,
clipboard or update client. The native production executable completes startup,
typing, Results and an acknowledged SQLite save while process-local IP networking
is denied. Positive unrestricted and negative TCP/bind/UDP controls verify the
restriction; required Unix IPC still works. This is
[denied-network operation](measurements/network-denied-permissions-final.json), not packet
capture or a claim that every possible path's syscalls were traced.

Stage A and pre-storage Stage B reports remain preserved with their original
compiler, OS, executable hashes and private/no-storage scope. Changed source,
storage and OS conditions prevent treating their deltas as controlled regressions.
The earlier integrated `stage-d-full.json` also remains under its original
source/executable identity. The final-package `stage-d-full-permissions-final.json`
is the current storage-enabled observation. The differing development-host
timings are not a controlled regression comparison.

Remaining external gates are native Linux/Windows and native Intel execution,
actual macOS Terminal/Windows Terminal/Kitty and baseline emulator appearance/input,
actual system sleep, genuine cold-cache local-SSD startup and an established
dedicated reference machine with repeatable regression comparisons. The harness
rejects unsupported baseline comparisons and flags repeatable increases of at
least 10% for investigation. No shared-CI timing threshold or invented hardware
result substitutes for those gates. [Methodology](benchmark-methodology.md),
[the terminal matrix](terminal-matrix.md), and [coverage](coverage.md) keep them
explicit.
