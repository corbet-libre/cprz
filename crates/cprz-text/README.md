# cprz-text

Smallest-output lossless compression for UTF-8 text.

Each payload is compressed twice — zstd at the highest level (optionally with
a caller-supplied dictionary) and Brotli at quality 11 with a large window —
and the smaller frame wins. A one-byte header records the choice, so
decompression needs no side information besides the dictionary (when one was
used). Ties go to zstd, which decodes faster.

## Packet format

One header byte followed by the codec frame:

| Header | Codec | Notes |
|---|---|---|
| `0x00` | zstd, no dictionary | Highest level (`zstd::compression_level_range` end, i.e. 22) |
| `0x01` | zstd, with dictionary | Caller must pass the same dictionary to `decompress` |
| `0x02` | Brotli | Quality 11, `lgwin` 24 (16 MiB window) |

Header values are stable and never reused. Dictionaries are trained on public
text, never on user plaintext; the dictionary bytes themselves are not stored
in the packet.

## Prior art evaluated (September 2026)

No maintained crate implements this specific scheme (best of zstd vs Brotli
with a self-describing header), so the selection layer is new. Both codecs it
selects between are established libraries; this crate is a thin facade over
them and contains no compression algorithm of its own.

### zstd back ends

| Candidate | Verdict |
|---|---|
| `zstd` 0.14 (`gyscos/zstd-rs`, bindings to Meta's reference C implementation) | **Chosen.** Full level range including 22, dictionary and long-distance-matching support, streaming decode with dictionaries, ~400M downloads. The earlier `wasm32-unknown-unknown` breakage (`zstd-sys`, issue #271) was fixed upstream; CI here proves the wasm build. BSD-3-Clause. |
| `ruzstd` 0.9 | Decoder only ("A decoder for the zstd compression format"); no encoder, so it cannot meet the encode side. MIT. |
| `libzstd-rs-sys` (Trifecta Tech Foundation, pre-release, June 2026) | Pure Rust but decompression and dictionary builder only; the encoder is still future work ("Milestone 4"). Revisit once it matures. |
| `structured-zstd` 0.0.x | Pure-Rust, all levels, dictionaries, wasm-ready — the most credible alternative. Rejected for now: `0.0.x` version signals an unstable API from a small single-org project, versus the byte-exact reference implementation. Revisit if the C toolchain ever blocks a target. Apache-2.0. |
| `zrip` | Pure Rust but levels −7..4 only; cannot do highest-level encoding. |

### Brotli back ends

| Candidate | Verdict |
|---|---|
| `brotli` 9 (`dropbox/rust-brotli`) | **Chosen.** Pure-Rust port of the reference encoder/decoder, maintained (9.0.0, September 2026), ~270M downloads, `wasm32-unknown-unknown` safe, `std` I/O wrappers on by default. BSD-3-Clause AND MIT. |
| `brotlic`, `compu-brotli-sys` | C bindings; strictly worse for wasm portability than the pure-Rust port. |
| `burli` | New from-scratch rewrite; not established. |

### Meta-codecs

`silesia`-style benchmark harnesses and HTTP content-negotiation layers pick
per-payload codecs, but no maintained crate offers a "compress with both,
keep the smaller, tag the winner" primitive for embedded payloads. The
literature supports the design: at the highest effort Brotli reaches smaller
sizes than zstd on text while zstd decodes faster, so trying both and keeping
the minimum dominates either codec alone.

## WebAssembly

Both dependencies build for `wasm32-unknown-unknown`: `brotli` is pure Rust,
and the `zstd` C sources compile to wasm via `cc` (proven by
`cargo check --workspace --target wasm32-unknown-unknown` in CI). No network,
no threads, no OS-specific calls on any path.

## License

LGPL-3.0-only WITH LGPL-3.0-linking-exception, like the rest of the
workspace. Dependency licenses (BSD-3-Clause, MIT) are compatible.
