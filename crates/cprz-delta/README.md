# cprz-delta

Compact deltas of a new payload version against a previous one: `diff(old,
new) -> Vec<u8>`, `apply(old, delta) -> Result<Vec<u8>, Error>`.

The delta path compresses `new` with `old` as a raw-content zstd dictionary
(patch-from style, long-distance matching on, highest level, content checksum
embedded). When that is not strictly smaller than `new` compressed on its own
(unrelated data, empty `old`), the standalone encoding is stored instead. The
first byte records the mode (`0x01` delta, `0x02` full), so every payload is
self-describing and `apply` needs no side information besides the `old`
version the delta was built against. `MODE_FULL` ignores `old` entirely. For
`MODE_DELTA`, `apply` decodes with `old` as the dictionary and fails on the
embedded content checksum whenever the differing bytes change the output; a
near-identical base whose differences the delta never references can still
decode, because the checksum covers the decoded content, not the identity of
the base.

## Scope / limits

Native zstd sublibrary over finite plain bytes: `diff`/`apply` take in-memory
slices and `apply` takes no limit parameter, so unlike the portable facade
this API cannot enforce a decoded-size/work budget; callers needing enforced
limits must use a bounded decoding path.

## Implementation survey (historical, September 2026)

Historical record, not a current runtime benchmark or wasm execution claim.
Per the workspace rule to search before writing, the candidates evaluated
(searched crates.io, GitHub and the docs in September 2026):

| Candidate | Scheme | License | Verdict |
|---|---|---|---|
| `zstd` (gyscos/zstd-rs 0.14) | Reference zstd bindings, bulk dictionary API | BSD-3-Clause | **Chosen**: established since 2016, ~400M downloads, maintained, documents `wasm32-unknown-unknown` builds (wasm shims in zstd-sys, wasm CI). The C dependency is acceptable because it builds for wasm32 in CI. |
| `oxidelta` 0.1.4 | VCDIFF (RFC 3284), xdelta3 reimplementation | MIT | Rejected: different scheme than the required zstd-dict/fallback design, brand-new 0.x single-maintainer code with heavy default dependencies. |
| `gdelta` 0.2.x | Own novel delta algorithm over lz4/zstd | MIT | Rejected: unproven custom algorithm; the product boundary says to wrap established libraries, not adopt new ones. |
| `delta-compression` 0.5.0 | Own rolling-hash/splay delta codec | BSD-2-Clause | Rejected: single release, ~20 downloads, unmaintained custom algorithm. |
| `xpatch` 0.4.x | Delta library with algorithm selection | AGPL-3.0-or-later | Rejected: full AGPL is excluded by the workspace license boundary. |
| `ruzstd` / `structured-zstd` / `zstd-pure-rs` | Pure-Rust zstd implementations | MIT/Apache-2.0 | Rejected: `ruzstd`'s encoder has no dictionary support (the one feature this crate needs); the others are new 0.x ports with no track record versus the reference implementation. |

No maintained crate implements exactly "zstd-dict delta with compressed
fallback and a mode header", so this crate is a thin shim over `zstd`'s
`bulk::Compressor::{new, with_dictionary}` plus streaming dictionary decode,
adding only the mode selection, the header byte, and the error type.

## Layout

- `src/lib.rs`: `diff`, `apply`, `Error`, `MODE_DELTA`, `MODE_FULL`, plus
  round-trip tests with size assertions (tiny deltas for small edits in large
  texts, fallback for unrelated data, empty/binary edge cases, malformed
  input rejection).
