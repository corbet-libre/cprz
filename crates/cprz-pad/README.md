# cprz-pad

Privacy-preserving padding using the Padmé size classes from Nikitin et
al., "Padded Uniform Random Blobs (PURBs)" (2019, Section 4).

API: `padded_len(len)`, `pad(data)`, `unpad(padded)` with an 8-byte
big-endian length prefix plus zero padding, so `unpad` recovers the exact
input. The `fixed` module offers power-of-two size classes for very
sensitive data. Overhead is at most about 12% in Padmé mode once the
header is amortised (lengths of 256 bytes and up; verified by tests up to
100000).

## Prior-art search (required)

| Candidate | Result | Verdict |
|---|---|---|
| `padme-padding` 0.1.1 (jedisct1, MIT, crates.io) | Purpose-built Padmé oracle, `no_std`, zero dependencies, reputable author, but untouched since Jan 2020 (5 commits) and, decisively, computes the exponent as `63 - len.leading_zeros()` on `usize`. On 32-bit targets — including the workspace-required `wasm32-unknown-unknown` — this underflows (debug panic) for small lengths, e.g. `padded_len(1)`, and returns the input unchanged for larger ones (e.g. 100 → 100 instead of 104). Verified by simulating `u32::leading_zeros` against the paper formula. | Rejected: wrong on a required target |
| `sequoia-openpgp` `serialize::stream::padding::padme` | Maintained, correct `u64` implementation with the same ≤12% bound. | Rejected: pulling all of sequoia-openpgp (dozens of crypto dependencies, optional C bindings) for one integer function violates the pure-Rust/wasm-friendly dependency policy |
| `padme-frame` 0.1.0 (crates.io) | Claims pad/unpad framing, but yanked, single release, unknown provenance. | Rejected: yanked and untrustworthy |
| `go-padme-padding` (jedisct1, Go) | Reference implementation of the same formula, incl. the `len <= 0 → 0` guard. | Used as a cross-check for the formula, not a dependency (different language) |

Decision: implement the ~10-line rounding formula directly over `u64`
(width-independent, identical results on native and wasm32), attributed to
the PURBs paper and the implementations above. No dependencies; the framing
(length prefix + zero fill), `Error` type, fixed power-of-two mode, and
tests are original to this crate, as no maintained crate provides that API.
