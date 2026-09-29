# cprz-structured

Most-compact encoding for structured records and messages.

Values are serialized with the [`bitcode`] crate's `serde` integration,
which packs booleans, small enums and bounded integers down to the bit
level, then compressed with zstd at the highest level when that shrinks the
payload further. A one-byte header records which form the packet holds, so
`unpack` needs no side information. This crate is a thin facade: it contains
no serialization algorithm of its own.

## Packet format

One header byte followed by the payload:

| Header | Encoding | Notes |
|---|---|---|
| `0x00` | `bitcode` (`serde` integration) | Stored directly; tiny records live here |
| `0x01` | zstd wrapping `bitcode` | Highest level (`zstd::compression_level_range` end, i.e. 22); kept only when strictly smaller |

Header values are stable and never reused. `pack` tries both forms and
keeps the smaller one, so small records cost exactly one header byte of
overhead while larger or repetitive batches gain the zstd frame. No
long-distance matching: it adds frame overhead that hurts the small payloads
this crate targets. Decoding needs no side information.

## `serde` shapes that work

`pack` accepts any `T: Serialize` that `bitcode`'s `serde` integration can
represent: plain structs and enums, `Option`, sequences, maps and strings.
It cannot represent `#[serde(untagged)]` or `#[serde(tag = ...)]` enums,
`#[serde(skip_serializing_if)]` / `#[serde(default)]` fields,
`serde_json::Value`-style dynamically typed values, or enums with more than
256 variants (`bitcode` is not self-describing; see its "Serde" wiki page).
Passing such a value makes `pack` panic; every other failure mode is an
`Error` from `unpack`. Plain records and messages — the payloads this crate
exists for — always work.

## Prior art evaluated (September 2026)

| Candidate | Verdict |
|---|---|
| `bitcode` 0.6.9 (SoftbearStudios, MIT OR Apache-2.0) | **Chosen.** Bit-level packing: booleans, small enums and bounded integers occupy single bits where possible, and fields are grouped so the output also compresses well. Pure Rust, `no_std` + `alloc`, `serde` behind a feature flag, MSRV 1.88. Smallest output in independent benchmarks (see below). |
| `postcard` 1.1.3 (jamesmunns, MIT OR Apache-2.0, ~16.8M downloads) | Runner-up. Same facade shape (byte-oriented `serde` codec, `no_std`, pure Rust, stable wire format since 1.0, maximal `serde` coverage) but byte-level: every bool and enum discriminant costs a full byte, so it is consistently a few bytes larger on small records. |
| `bincode` 2.0.1 / 3.0.0 | Rejected: upstream marked the crate unmaintained (December 2025 notice on lib.rs). Fixed-width integers also make it larger than both candidates above on small records. |
| `rmp-serde` 1.3.x (MessagePack, MIT) | Rejected: maintained and pure Rust, but every value carries a type tag and integers use MessagePack's fixed type widths, so small records are larger. |
| CBOR (`ciborium` 0.2.2, `cbor4ii` 1.2.3) | Rejected: self-describing major-type tags on every item cost bytes on small records; `ciborium`'s maintenance cadence has also slowed (last release early 2024). |
| `prost` 0.14.x (Protocol Buffers, tokio-rs) | Rejected: schema-first (`.proto` files, `protoc` build dependency), per-field tags, and poor `serde` derive interop — the wrong shape for a generic `pack<T: Serialize>` facade. |
| `capnp` 0.27.x (Cap'n Proto, incl. packed encoding) | Rejected: schema compiler plus `build.rs` codegen, 8-byte word alignment, and pointer overhead make packed messages larger than `bitcode` on small records. Its zero-copy strength does not apply here, where decoding into owned values is the goal. |

The zstd back-end choice (`zstd` 0.14, bindings to Meta's reference C
implementation, BSD-3-Clause) is shared with `cprz-blob`; see its README.
It is used here only as an optional second stage, at the highest level, with
no long-distance matching.

## Size comparison

Illustrative sizes for the profile-like record used in the test suite
(two booleans, a 3-variant enum, a `u8`, a `u32`, two short strings and an
`Option<String>`). JSON is exact (`serde_json`: field names plus
punctuation, 105 bytes); the binary figures are approximate, from format
analysis and the published benchmarks cited below. The ordering — not the
exact bytes — is the point, and CI tests pin the load-bearing claims:
`pack` output is less than half the JSON size on the sample record and below
the raw `bitcode` size on batches.

| Format | Why this size | Typical size (bytes) |
|---|---|---|
| JSON (`serde_json`) | Field names repeated, punctuation, enum as string | 105 |
| This crate (`bitcode` + header) | No field names; bools/enum take bits; varint-scale integers; strings length-prefixed | ~21 |
| `postcard` | No field names, but every bool and the enum discriminant costs a byte | ~28 |
| `bincode` 2 | No field names, but fixed-width integers (`u32` = 4 bytes) | ~40 |
| MessagePack (`rmp-serde`) | Array form (no field names) but a type tag per value | ~45 |
| CBOR | Like MessagePack plus major-type tags | ~45 |
| Protocol Buffers (`prost`) | Per-field tags + varints; no field names on the wire | ~40 |
| Cap'n Proto packed | Word-aligned struct with pointers, then packed encoding | ~50+ |

Published evidence for the ordering: the
[`rust_serialization_benchmark`](https://github.com/djkoloski/rust_serialization_benchmark)
suite (bitcode smallest and fastest among compact `serde` codecs),
[zeenix/json-vs-bin](https://github.com/zeenix/json-vs-bin) (bitcode 18.5%
of JSON size vs postcard 22.8% and bincode 24.2% on struct payloads), and the
[musli benchmarks](https://udoprog.github.io/musli/benchmarks) (bitcode at or
below postcard sizes).

## WebAssembly

Builds for `wasm32-unknown-unknown`: `bitcode` and `serde` are pure Rust
(`bitcode` needs only `alloc` beyond core), and the `zstd` C sources compile
to wasm via `cc` (proven by
`cargo check --workspace --target wasm32-unknown-unknown` in CI). No network,
no threads, no OS-specific calls on any path.

## License

LGPL-3.0-only WITH LGPL-3.0-linking-exception, like the rest of the
workspace. Dependency licenses (MIT OR Apache-2.0, BSD-3-Clause) are
compatible.

[`bitcode`]: https://crates.io/crates/bitcode
