# Stage A measurements and scoring decisions

Measured locally on September 4, 2026, on Apple M3, 8 logical CPUs, 16 GiB RAM, macOS 26.5.2 (25F84), arm64. Rust 1.95.0 (59807616e), Cargo 1.95.0. Release profile uses the portable aarch64-apple-darwin target, optimization level 3, thin LTO, one codegen unit, symbol stripping, and unwind panics. This is a development machine, not yet an established controlled reference machine.

`cargo run --offline --release --example kernel_bench` measured 50,000 deterministic ASCII reducer actions including wrong attempts, correction, word submission, and bucket advancement. Instrumentation uses `Instant` around each `apply` call. Target preparation and history are outside the measured interval. No terminal is involved; terminal type, geometry, local/remote rendering, and history size are not applicable to this kernel-only measurement.

| Metric | Measured |
|---|---:|
| Median apply | 125 ns |
| p95 apply | 250 ns |
| p99 apply | 958 ns |
| Heap allocations in measured reducer interval | 0 |
| Bytes allocated in measured reducer interval | 0 |

These results meet the local reducer target of p99 ≤50 µs and zero warmed ordinary ASCII allocations. They do not establish startup, rendering, real terminal latency, physical key-to-photon latency, CPU, or resident-memory targets. Absolute timings must be remeasured on controlled reference hardware before publishing a performance claim.

`cargo test --offline --test kernel_performance` passed three tests: warmed ordinary ASCII insertion/correction/backtracking/bucket updates allocate nothing; 50,000 zen units cycle through a 4,096-grapheme retained window without losing cumulative counters; 1,000 successive engine runs and result snapshots leave zero net tracked heap allocations.

Scoring decisions made explicit during the specification audit:

- Prose always compares in NFC. An inactive exact normalization preference does not split prose records. Exact normalization remains an effective profile parameter.
- Blind error styling is cosmetic for profile identity; matching and challenge rules are unchanged.
- Missed units represent omitted target suffixes at submitted tokens. Untouched timed lookahead and the unsubmitted suffix of the active timed token are not counted as missed.
- An action completing at a full-second endpoint is reconciled into that final bucket, so its final cumulative counts agree with the result. There is no fictitious fractional bucket.
- Pending canonical graphemes finalize at the scoring endpoint even when receipt of the deadline watermark is delayed. A challenge failure discovered there keeps that endpoint.
- Random profile content identity uses the selected pack's content hash. The seed and generation version reproduce a particular sample, while the seed does not split record categories. Explicit seeds and repeated samples are practice.
- Zen `current` and `mistakes` correction stop at a submitted whitespace boundary, since free writing has no incorrect target token to reopen. `full` reaches all retained scrollback, and `none` disables deletion. Discarded scrollback is never editable.
- Random time/words and quotation modes use prose policy. Literal exact-text tests use custom/code profiles, preventing an early F5 confirmation from being categorized as a completed timed record.
