# cprz-audio

Voice and sound at the smallest size: mono Opus at 6–16 kbps (16 or 48 kHz)
in an Ogg Opus container (RFC 7845), so the output plays in any Ogg
Opus capable player.

`encode_voice` takes mono `i16` PCM at the configured rate, encodes 20 ms
frames at constant bitrate with the highest encoder effort, and frames the
packets in Ogg with `OpusHead` plus `OpusTags` headers. `decode_voice` reads
the sample rate back from `OpusHead`, so decoding needs no side information.
Opus is lossy: decoding returns the same number of samples (the final partial
frame is padded with silence on encode) with slightly different values.

## Packet format

Standard Ogg Opus, mono, mapping family 0:

| Packet | Contents |
|---|---|
| `OpusHead` (19 bytes) | Magic, version 1, 1 channel, pre-skip 0, input rate, gain 0, mapping 0 |
| `OpusTags` | Magic, vendor `cprz-audio`, zero user comments |
| Audio | One CBR Opus packet per 20 ms frame, granule positions in 48 kHz samples |

Pre-skip is 0: the encoder delay is not trimmed, so the decoded samples line
up one-to-one with the padded input. The Ogg serial number is fixed, which
keeps encoding deterministic. A future revision may report the real lookahead
once the codec exposes it.

## Prior art evaluated (September 2026)

### Opus encoders

| Candidate | Verdict |
|---|---|
| `opus-rs` 0.1 (`restsend/opus-rs`, pure-Rust port of libopus 1.6) | **Chosen.** Encoder and decoder, no C, zero dependencies in the `std` build, maintained (0.1.34, September 2026), explicitly verified on `wasm32-unknown-unknown`, `#![no_std]` capable. Public `bitrate_bps` / `complexity` / `use_cbr` controls plus `encode_i16` for direct PCM16 input. BSD-3-Clause. |
| `opus` 0.4 (`SpaceManiac/opus-rs`, safe bindings over `opusic-sys`/libopus C) | Rejected: the reference encoder behind a C toolchain. Correct and maintained (August 2026), but a C dependency risks the `wasm32-unknown-unknown` CI check for no audible gain over the pure-Rust port. MIT/Apache-2.0. |
| `audiopus` 0.2–0.3-rc | Rejected: historically popular but stale (last release April 2021), binds the system libopus, needs installed headers/`cmake`. Not maintained. |
| `ruopus` 0.1 | Rejected: promising pure-Rust codec (MIT, mid-2026) but new with a tiny user base and less proven encoder quality; revisit once it matures. |
| `opus-decoder` 0.1, `opuscule` 0.2 | Rejected: decoders only, no encode side. |
| `libopusenc` 0.2 (bindings to reference `libopusenc`) | Rejected: convenient Ogg Opus file API but a C library with negligible adoption; the hand-rolled two headers over pure-Rust `ogg` cover the same ground with no C. |
| `ogg-opus` 0.1, `opus_headers` 0.1 | Rejected: stale (2020–2021). Ogg framing comes from `ogg` instead; the two Opus headers are ~30 lines per RFC 7845. |

### Ogg container

| Candidate | Verdict |
|---|---|
| `ogg` 0.9 (`RustAudio/ogg`) | **Chosen.** Pure-Rust Ogg packet reader/writer, maintained, ~10M downloads, `wasm32-unknown-unknown` safe. BSD-3-Clause. |

### Neural speech codecs (survey only — none implemented)

| Codec | Bitrate | Code / weights licence | Runtime | Verdict |
|---|---|---|---|---|
| Mimi (Kyutai, streaming, 12.5 Hz tokens) | ~1.1 kbps | Code Apache-2.0, weights CC-BY | PyTorch; no maintained Rust inference crate on crates.io | Not implemented: needs a model runtime plus a weight download, and the CC-BY weights complicate embedding. Most interesting future direction. |
| Lyra v2 (Google, 3.2 / 6 / 9.2 kbps) | 3.2–9.2 kbps | Apache-2.0 | C++ with Bazel; no maintained Rust crate | Not implemented: no Rust entry point to wrap. |
| EnCodec (Meta, 24 kHz mono / 48 kHz stereo) | ~1.5–24 kbps | Code permissive, **weights CC-BY-NC 4.0** | PyTorch | **Excluded by licence alone**: non-commercial weights cannot ship in this LGPL library. |
| DAC (Descript, 44.1/24/16 kHz) | ~2–14 kbps | MIT code and weights | PyTorch | Licence is fine, but there is no maintained pure-Rust/torch-free inference crate; revisit if one appears. |

Recommendation: stay with Opus for now — universally playable, no model
weights, no GPU/runtime dependency, and 6–16 kbps already covers voice notes.
Revisit Mimi first (lowest bitrate, Apache code) once a pure-Rust inference
path (candle/burn/ort) with downloadable weights matures; DAC second on the
same condition; Lyra if a cargo crate appears; never EnCodec under its
non-commercial weights. Any neural addition must stay behind a default-off
feature until it proves `wasm32-unknown-unknown` clean.

## WebAssembly

Both dependencies are pure Rust and build for `wasm32-unknown-unknown`
(`opus-rs` lists the target as verified; `ogg` has no platform-specific
code), so no feature gating is needed: browsers that prefer their own Opus
via WebCodecs can still exchange these bytes, since the container is standard
Ogg Opus. No network, no threads, no OS-specific calls on any path.

## License

LGPL-3.0-only WITH LGPL-3.0-linking-exception, like the rest of the
workspace. Dependency licenses (BSD-3-Clause) are compatible.
