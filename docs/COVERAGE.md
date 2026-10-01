# Coverage contract

CI targets 100% of reachable production lines and branches. Stable Rust runs
the existing native and wasm checks. Nightly Rust is used only for LLVM branch
instrumentation, which currently requires it. Both jobs use the same resolved
Cargo.lock snapshot; all build and test commands after resolution use --locked.

The coverage job executes real tests with cargo-llvm-cov and retains the raw JSON
even when the gate fails. The checker compares integer covered/total counts for
both metrics; rounded percentages cannot pass. An empty report cannot pass.
The report excludes integration-test harness files under tests/, not production
code. No production source exclusions are currently approved.

A failing gate is missing evidence, not permission to lower the threshold or
change domain behavior. Add meaningful failure and round-trip tests. Document
any genuinely unreachable defensive branch precisely before excluding it. Native
coverage does not establish browser execution; keep the actual wasm vectors.

First-party dependencies follow main. Their resolved full revisions remain in
Cargo.lock, with exactly one source per first-party crate. Dependabot maintains
committed snapshots; CI refreshes once per run and retains the tested snapshot.
Auto-merge requires protected main and successful substantive checks on the
exact current Dependabot head. It never executes PR code with write permissions.

## Emitted source metric

The acceptance metric is now 100% of upstream LLVM LCOV's emitted production
source line counters (DA) and branch counters (BRDA), from the same execution as
the retained raw JSON. This is source coverage, not every generic instantiation.
LLVM JSON and LCOV summary totals can count generic copies differently from
the merged source records. The original JSON checker remains diagnostic code;
its stricter instantiation totals are not described as passed.

The source gate requires a nonempty report, an exact file inventory matching the
companion JSON, matching raw summary metadata, complete emitted branch counts
and no duplicate, unknown or zero counters. The raw inventory retains all production counters; only the exact proposals below are subtracted after full inventory reconciliation.
Actual browser/device execution remains separate from native coverage.

## Proposed native exceptions

These are source-bound review proposals, not security acceptance or a claim that
all possible failures have been reproduced. Baseline
[bcec66f / 36940806989](https://github.com/corbet-foss/cprz/actions/runs/36940806989)
passed the native workspace and actual facade/blob/Padme Wasm vectors. Its
unmodified raw reports contain 1,623/1,632 production lines and 254/256 branch
arms. Every text, audio, image, blob, delta, structured and facade line/arm ran.
All 12 public control-character corpora selected the actual smallest codec frame
and round-tripped; the ordinary Zstd selector and decoder now execute as well.

The native coverage commands explicitly select `x86_64-unknown-linux-gnu`.
Target restrictions are part of the proof, not a claim about other targets.
The manifest binds each exact coordinate and entire owned source file plus the
coverage workflow. Upstream-dependent proposals additionally bind the complete
Cargo.lock, including rav1e 0.8.1's registry checksum. Dependency updates must
renew that evidence; no declaration or version range is pinned to preserve it.

### Integer representation (six lines, one arm)

- Padme `src/lib.rs:148-150` belongs only to the failed `u64` to `usize`
  conversion closure. On the explicit 64-bit target every u64 is representable.
  Actual wasm32 tests separately submit `u32::MAX + 1` and `u64::MAX` length
  prefixes and require refusal without allocating the declared data. These lines
  are not excluded from a 32-bit target or treated as browser coverage.
- Video `src/lib.rs:530,566,579` and line 529/block 0/arm 1 are top-level
  checked-add overflow guards. `read_element` permits at most four ID bytes and
  eight size bytes; the all-ones size is classified as unknown. Thus finite body
  sizes are at most `2^56 - 2`, headers at most 12, an EBML end at most
  `2^56 + 10`, and a finite Segment end at most `2^57 + 20`. All fit usize64.
  Other truncation, parent-overrun, unknown-size and malformed-element refusals
  are reachable and run. These proposals make no claim about 32-bit video.

### Upstream single-pass invariants (two lines, one arm)

Video line 399 returns unhandled upstream receive errors. The configured encoder
uses one pass: `Config::default` leaves both two-pass options off, and `RCState`
starts in PASS_SINGLE, where `ready()` is always true. Therefore receive cannot
return NotReady. Its remaining Failure path is `finalize_packet` calling
`write_temporal_delimiter` on the encoder's owned `Vec<u8>`. That helper only
writes a fixed delimiter with `write_all`; Vec writes return success (allocation
failure aborts or panics, rather than returning this I/O error). The handled
Encoded, NeedMoreData and LimitReached states remain measured.

Video line 404 and line 403/block 0/arm 0 defend against an empty output packet
list. The facade rejects empty input before constructing the encoder. Frame zero
is a shown keyframe, starts at input index zero below the positive submitted
frame limit, and is finalized into a packet before completion. The other
completion path compares processed frames with that positive limit; the bound
upstream increments processed frames only while finalizing a returned packet.
Configuration and frame-shape failures return earlier and are exercised with
actual upstream refusals. No fabricated encoder or own-logic mock is used.

### Unqualified extreme resource failure (one line)

**Video line 387 is not a logical-unreachability claim.** The upstream
`Context::send_frame` refuses after flushing or a still picture's first frame.
This facade flushes only after submission and uses video mode. The remaining
route starts automatic flushing at the upstream i32 frame-count limit and then
refuses the next submission: reaching it needs at least `2^31` actual frames.
The facade's input vector and retained upstream frames make that a very large
resource experiment, not a bounded codec unit test. This single mapping line is
proposed as a resource exclusion, explicitly without failure-path qualification.
It requires maintainer review; the reports must never be described as testing
that refusal. No frame limit was added to domain behavior to hide it, no
upstream state was fabricated, and no production failure arm was mocked.

The original JSON, LCOV and annotated reports are retained unchanged. The gate
requires all 1,623 nonexcluded lines and 254 nonexcluded arms and runs seventeen
regressions against malformed inventories and unsupported exception evidence.
The native integer and upstream proofs are separate from the resource exception.
Actual whole-workspace multimedia Wasm execution is not claimed.

Primary implementation evidence, verified against the lockfile crate checksum:
[rav1e context](https://docs.rs/crate/rav1e/0.8.1/source/src/api/context.rs),
[frame lifecycle](https://docs.rs/crate/rav1e/0.8.1/source/src/api/internal.rs),
[single-pass state](https://docs.rs/crate/rav1e/0.8.1/source/src/rate.rs), and
[delimiter and first keyframe](https://docs.rs/crate/rav1e/0.8.1/source/src/encoder.rs).
