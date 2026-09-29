# cprz-video

Short video clips: encode raw RGB or YUV 4:2:0 frames as AV1 in a WebM
container browsers can play.

This crate is a thin facade over a maintained library. AV1 encoding uses
`rav1e` (the pure-Rust Xiph encoder). The minimal WebM (EBML) muxer, the
RGB-to-YUV converter and the container-level probe are the only logic
owned by this crate. There is no pixel-level decoder yet; [`probe`] reads
dimensions and frame count back from the container instead.

## Dependency survey

Evaluated September 2026, per the workspace "search before you write" rule.

| Task | Candidates evaluated | Chosen | Why |
|---|---|---|---|
| AV1 encode (pure Rust, wasm32) | `rav1e` 0.8.1 (Xiph, BSD-2-Clause, pure Rust, explicit `wasm` feature, docs.rs builds with `no-default-features`; powers `ravif`/`cavif`) — `libaom` bindings (C library, fails the wasm32 rule) — SVT-AV1 bindings (C library, fails the wasm32 rule) — `zenrav1e` (rav1e fork tuned for still AVIF, less established) | `rav1e` 0.8, `default-features = false` | The only maintained pure-Rust AV1 encoder; builds for native and `wasm32-unknown-unknown` without the NASM assembler (the default `asm` feature is off, the pure-Rust fallback emits the same bitstream, only slower) |
| AV1 decode | `rav1d` 1.1.0 (memorysafety Rust port of dav1d, BSD-2-Clause, but `build.rs` needs `nasm-rs` + `cc` and the API surface is C-level) — `dav1d-sys` / `libdav1d-sys` (C bindings, fail the wasm32 rule) — `rav1d-safe` 0.6.0 (native Rust API, but AGPL-3.0-only, forbidden by the workspace license) — `zenavif` (wraps `rav1d-safe`, inherits AGPL) | none | No decoder is simultaneously maintained, wasm32-buildable without extra toolchains, and license-compatible; browsers decode WebM/AV1 natively, so pixel decode is left to them |
| WebM mux (pure Rust, wasm32, AV1) | `matroska` 0.30 (metadata/demux only, no muxer) — `oxideav-mkv` 0.0.10 (brand-new September 2026, unproven, coupled to the `oxideav-core` runtime) — `media-format-matroska` / `rff-format-mkv` (demux only) — `webm` crate (VP8/VP9 era, no AV1 path) | own minimal EBML muxer | No maintained pure-Rust WebM muxer with AV1 support found; writing the established container per the WebM guidelines is not inventing a format. CodecPrivate carries `rav1e`'s `container_sequence_header` so players initialise before the first frame |
| AV2 | Spec 1.0 released 28 May 2026; `rav2d` experimental Rust decoder only; no browser decoder, no maintained Rust encoder | none | Revisit once decoders exist (crate description scope already says "later AV2") |

License check: `rav1e` (BSD-2-Clause) is compatible with the workspace
license (LGPL-3.0-only WITH LGPL-3.0-linking-exception). No GPL/AGPL code.

## API

- `encode(clip, options)`: encode a [`Clip`] (width, height, frame rate,
  raw [`RawFrame`]s in RGB8 or YUV420) as AV1 in WebM. `RateControl`
  selects constant quality (quantizer 0..=255) or capped bitrate (kbit/s);
  `speed` is the `rav1e` preset 0..=10 (0 compresses best, runs slowest).
  [`EncodeOptions::default`] is constant quality at high-effort speed.
- `encode_rgb(frames, width, height, fps_num, fps_den, options)`:
  convenience entry point for packed RGB frames.
- `probe(data)`: read [`VideoInfo`] (dimensions, frame count) back from
  the container without decoding pixels.
- `is_webm(data)`: check the EBML magic plus the `webm` document type.
- `EncodedVideo::dimensions`, `byte_size`, `frame_count`, `info`:
  output shape and size.

## Platforms

Native and `wasm32-unknown-unknown`. The only dependency is pure Rust
with no C build step. Full pixel-level decoding is future work, pending a
maintained, license-compatible AV1 decoder that builds for wasm32.

## License

Same as the workspace: LGPL-3.0-only WITH LGPL-3.0-linking-exception.
