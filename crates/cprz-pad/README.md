# cprz-pad

Privacy-preserving padding using the Padmé size classes from Nikitin et
al., "Padded Uniform Random Blobs (PURBs)" (2019, Section 4).

## Framing (exact)

Every padded buffer has the same layout, in both modes (see
[src/lib.rs](src/lib.rs)):

```text
u64 BE data length | data bytes | zero fill to the size class
<--- HEADER_LEN --->|<-- exact input -->|<-- zero bytes -->|
```

- `HEADER_LEN: usize = 8`. The buffer starts with the original data length
  encoded as an 8-byte big-endian `u64`, followed by the data itself; the
  remainder of the size class is zero bytes (`frame` allocates a zeroed
  buffer, then writes prefix plus data).
- `pad` output length always equals `padded_len(data.len())`.
- `unpad` reads the prefix back and returns exactly the declared data bytes,
  so `unpad(pad(x)) == x`. One decoder handles both modes: top-level
  `unpad` decodes `fixed::pad` output and `fixed::unpad` (which delegates to
  it) decodes `pad` output.

## Modes

Two modes share the same framing; you pick by calling the functions you
need:

| Mode | Size oracle | Overhead oracle | Pad | Unpad |
|---|---|---|---|---|
| Default Padmé | `padded_len(len)` | `padding_len(len)` | `pad(data)` | `unpad(padded)` |
| Fixed power-of-two (`fixed` module, for very sensitive data) | `fixed::padded_len(len)` | `fixed::padding_len(len)` | `fixed::pad(data)` | `fixed::unpad(padded)` |

- Default rounds the framed length (`len + HEADER_LEN`) up to its Padmé
  class, computed over `u64` so it does not depend on pointer width. Native
  and `wasm32` results are identical wherever the rounded value is
  representable as `usize`; above that the public oracle saturates to
  `usize::MAX` (target-dependent, see below). Overhead of the pure oracle
  stays within about 12%; with the 8-byte header included the ~12% bound
  holds once the header is amortised (lengths of 256 bytes and up, checked
  up to 100000 in [src/tests/lib.rs](src/tests/lib.rs)).
- `fixed` rounds the framed length up to the next power of two, so only
  `O(log M)` distinct output lengths exist. Overhead can approach 100% just
  above a class boundary.
- `FIXED_MAX_FRAMED: u64 = 1 << 63` bounds the exact fixed range: framed
  lengths above it saturate (see below).

The public size oracles are not idempotent: each application of
`padded_len` re-adds `HEADER_LEN` before rounding, so
`padded_len(padded_len(n))` generally lands in a strictly larger class
(e.g. `padded_len(0) == 8` but `padded_len(8) == 16`). The fixed-point
property belongs to the private framed-length oracle only, which is a fixed
point of the public oracle's outputs (asserted in
[src/tests/lib.rs](src/tests/lib.rs)).

Reference vectors (from [src/tests/lib.rs](src/tests/lib.rs); framed = data
length + 8):

- Padmé `padded_len`: 0 → 8, 1 → 10, 2 → 10, 3 → 12, 8 → 16, 9 → 18,
  15 → 24, 16 → 24, 17 → 26, 25 → 36, 33 → 44, 92 → 104, 121 → 144,
  1000 → 1024, 100000 → 100352.
- Fixed `fixed::padded_len`: 0 → 8, 1 → 16, 8 → 16, 9 → 32, 100 → 128.

## Size oracles vs checked operations

- `padded_len(len)` / `fixed::padded_len(len)`: pure size-class oracles.
  They never allocate and never fail. Results agree across targets wherever
  the rounded value fits the target's `usize`; for values that do not fit
  (`u64` classes above the target's `usize`, and framed lengths above
  `FIXED_MAX_FRAMED` in fixed mode) the result saturates to `usize::MAX` —
  the saturated value is target-dependent. [src/tests/lib.rs](src/tests/lib.rs)
  asserts `padded_len(usize::MAX) == usize::MAX` and the same for `fixed`.
- `padding_len(len)` / `fixed::padding_len(len)`: `padded_len(len)` minus
  `len` via saturating subtraction (prefix plus zero fill). Size-only, same
  saturation behaviour, never an error.
- `pad(data)` / `fixed::pad(data)`: build the real buffer (always
  `#[must_use]`). Length equals the corresponding oracle for the input
  length.
- `unpad(padded)` / `fixed::unpad(padded)`: checked decode returning
  `Result<Vec<u8>, Error>`. The declared length is compared against the
  buffer before any output allocation, so oversized declarations are
  refused without allocating them; on success the declared bytes are then
  copied out of the buffer into the returned `Vec`.

## Errors and validation

`Error` ([src/lib.rs](src/lib.rs)) has three variants; its `Display` strings
name the prefix problem, the declared/available counts, or the nonzero
tail:

- `Error::Empty`: input is shorter than `HEADER_LEN`, so it holds no length
  prefix. `unpad(&[])` and any input under 8 bytes (e.g. `[0u8; 7]`)
  return this.
- `Error::InvalidLength { declared, available }`: the `u64` from the prefix
  does not fit the buffer (`data_len > available`, where
  `available = padded.len() - HEADER_LEN`). A `u64` that does not fit
  `usize` is also refused here without allocating.
  [tests/portable.rs](tests/portable.rs) submits `u32::MAX + 1` and
  `u64::MAX` prefixes on an 8-byte buffer and requires refusal with
  `available: 0`. Truncation into the data area also lands here (e.g.
  `unpad(&pad(b"hello")[..10])` gives `declared: 5, available: 2`).
- `Error::NonZeroPadding`: at least one byte after the declared data is not
  zero. Flipping the last byte of `pad(b"hello")` to `0xff`/`255` is
  refused this way.

Empty input is valid framed data, not an error at the `pad` layer:
`pad(&[])` is the 8-byte encoding of length 0 (`padded_len(0) == 8`) and
`unpad` recovers the empty vector. Only inputs shorter than the prefix hit
`Error::Empty`.

## Round-trip examples

The examples below mirror the assertions in
[src/tests/lib.rs](src/tests/lib.rs) and
[tests/portable.rs](tests/portable.rs):

```rust
use cprz_pad::{fixed, pad, unpad};

// Default Padme mode.
let padded = pad(b"hello");
assert_eq!(padded.len(), cprz_pad::padded_len(5));
assert_eq!(unpad(&padded), Ok(b"hello".to_vec()));

// Fixed power-of-two mode; either unpad entry point decodes either mode.
let fpadded = fixed::pad(b"hello");
assert_eq!(fpadded.len(), fixed::padded_len(5));
assert_eq!(unpad(&fpadded), Ok(b"hello".to_vec()));
assert_eq!(fixed::unpad(&fpadded), Ok(b"hello".to_vec()));

// Empty input round-trips; short and tampered inputs are refused.
use cprz_pad::Error;
assert_eq!(unpad(&pad(&[])), Ok(Vec::new()));
assert_eq!(unpad(&[]), Err(Error::Empty));
assert_eq!(unpad(&[0u8; 7]), Err(Error::Empty));
let mut tampered = pad(b"hello");
*tampered.last_mut().unwrap() = 255;
assert_eq!(unpad(&tampered), Err(Error::NonZeroPadding));
let declared = u64::MAX.to_be_bytes();
assert!(matches!(
    unpad(&declared),
    Err(Error::InvalidLength { declared: u64::MAX, available: 0 })
));
```

## What padding does and does not do

- Padding shrinks length resolution only: Padmé leaks `O(log log M)` bits
  about the length, the fixed mode exposes only `O(log M)` distinct output
  lengths. That is length hiding, not encryption or authentication.
- `unpad` validates the prefix fit and the zero tail; it does not detect
  substitution inside the declared data range, and padded output is not a
  commitment, signature, or ciphertext.
- No undetectability, canonicity across modes, or exact bucket-distribution
  promises are made: the two modes intentionally produce different classes,
  and only the documented vectors and bounds above are claimed.

## Portability notes (static, from manifests and CI config)

- Zero dependencies ([Cargo.toml](Cargo.toml) lists only a
  `cfg(target_arch = "wasm32")` dev-dependency on `wasm-bindgen-test`);
  workspace lints forbid `unsafe_code`.
- The size classes are computed over `u64`, independent of pointer width;
  public results agree across targets only where the rounded value is
  representable as `usize`, above which they saturate to `usize::MAX`
  (target-dependent).
- CI ([.github/workflows/ci.yml](../../.github/workflows/ci.yml)) runs the
  native workspace tests and, on `wasm32-unknown-unknown`, the `portable`
  test target for `cprz-pad` (which covers exact framing plus oversized
  prefixes); the workspace rule in
  [AGENTS.md](../../AGENTS.md) additionally requires `cargo check` for
  `wasm32-unknown-unknown`. This README claims no runtime or wasm
  execution; that evidence lives in the CI runs.

## Prior-art search (required by workspace AGENTS.md)

| Candidate | Result | Verdict |
|---|---|---|
| `padme-padding` 0.1.1 (jedisct1, MIT, crates.io) | Purpose-built Padmé oracle, `no_std`, zero dependencies, reputable author, but untouched since Jan 2020 (5 commits) and, decisively, computes the exponent as `63 - len.leading_zeros()` on `usize`. On 32-bit targets — including the workspace-required `wasm32-unknown-unknown` — this underflows (debug panic) for small lengths, e.g. `padded_len(1)`, and returns the input unchanged for larger ones (e.g. 100 → 100 instead of 104). Verified by simulating `u32::leading_zeros` against the paper formula. | Rejected: wrong on a required target |
| `sequoia-openpgp` `serialize::stream::padding::padme` | Maintained, correct `u64` implementation with the same ≤12% bound. | Rejected: pulling all of sequoia-openpgp (dozens of crypto dependencies, optional C bindings) for one integer function violates the pure-Rust/wasm-friendly dependency policy |
| `padme-frame` 0.1.0 (crates.io) | Claims pad/unpad framing, but yanked, single release, unknown provenance. | Rejected: yanked and untrustworthy |
| `go-padme-padding` (jedisct1, Go) | Reference implementation of the same formula, incl. the `len <= 0 → 0` guard. | Used as a cross-check for the formula, not a dependency (different language) |

Decision: implement the ~10-line rounding formula directly over `u64`
(identical on native and wasm32 wherever the rounded value is representable
as `usize`), attributed to the PURBs paper and the implementations above. No dependencies; the framing
(length prefix + zero fill), `Error` type, fixed power-of-two mode, and
tests are original to this crate, as no maintained crate provides that API.
