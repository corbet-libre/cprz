# cprz-blob

Smallest-output lossless compression for arbitrary bytes.

Payloads are compressed with zstd at the highest level with long-distance
matching (LDM) enabled, which links repeated blocks far apart in large inputs
such as backups, bundles and dumps. A one-byte header identifies the codec,
so decompression needs no side information; LDM is an encoder-side setting
stored in the frame itself.

## Packet format

One header byte followed by the zstd frame:

| Header | Codec | Notes |
|---|---|---|
| `0x00` | zstd | Highest level (`zstd::compression_level_range` end, i.e. 22) with `long_distance_matching(true)` |

Header values are stable and never reused; further values are reserved for
future codecs (e.g. a dictionary variant mirroring `cprz-text`).

## Prior art evaluated (September 2026)

This crate wraps an established library and contains no compression algorithm
of its own. The zstd back-end choice is shared with `cprz-text`; see its
README for the full table. In short:

- **Chosen: `zstd` 0.14** (`gyscos/zstd-rs`, bindings to Meta's reference C
  implementation). It exposes exactly what this crate needs —
  highest-level encoding plus `Compressor::long_distance_matching` — with
  transparent decoding, and its `wasm32-unknown-unknown` build was fixed
  upstream in 2024. CI here proves the wasm build on every push.
  BSD-3-Clause.
- Rejected: `ruzstd` 0.9 (decode-only), `libzstd-rs-sys` (pre-release,
  decoder only), `structured-zstd` 0.0.x (promising pure-Rust
  reimplementation, but `0.0.x` and not yet established), `zrip` (levels
  −7..4 only, no highest-level encoding).

No maintained crate offers "zstd-LDM at max level with a self-describing
one-byte header" as a primitive, so that thin wrapper is this crate.

## WebAssembly

Builds for `wasm32-unknown-unknown`: the `zstd` C sources compile to wasm via
`cc` (proven by `cargo check --workspace --target wasm32-unknown-unknown` in
CI). No network, no threads, no OS-specific calls on any path.

## License

LGPL-3.0-only WITH LGPL-3.0-linking-exception, like the rest of the
workspace. The `zstd` dependency is BSD-3-Clause, which is compatible.
