# cprz-image

Photos and graphics: decode PNG, JPEG or WebP, resize to display size, encode AVIF.

This crate is a thin facade over maintained libraries. It contains no codec
of its own: decoding and resizing use the `image` crate, AVIF encoding uses
`ravif` (pure-Rust AV1 encoder `rav1e` plus `avif-serialize`). A target byte
size is hit by binary search over the encoder quality; that loop is the only
logic owned by this crate.

## Dependency survey

Evaluated September 2026, per the workspace "search before you write" rule.

| Task | Candidates evaluated | Chosen | Why |
|---|---|---|---|
| Decode PNG/JPEG/WebP | `image` 0.25 (image-rs, MIT/Apache, ~199M downloads, pure-Rust decoders: `png`, `zune-jpeg`, `image-webp`); `load_image` (no WebP support); wiring `png` + `zune-jpeg` + `image-webp` by hand | `image`, features `png`, `jpeg`, `webp` | Established, maintained, pure Rust, builds for wasm32; single dependency covers all three formats |
| Resize with a high-quality filter | `image::imageops::resize` (Lanczos3, already a dependency); `fast_image_resize` 6.1 (Cykooz, MIT, ~20M downloads, SIMD, wasm-compatible) | `image::imageops::resize` | High-quality filter without adding a dependency; `fast_image_resize` is faster but unnecessary at display sizes |
| AVIF encode (pure Rust, wasm32) | `ravif` 0.13 (kornelski, BSD-3-Clause, ~49M downloads, powers `cavif`, also used by `image` itself); `image` `avif` feature (`AvifEncoder`, wraps `ravif` but takes quality as `u8` and hides the encoder); `libavif-sys`/`libavif` (C library, fails the wasm32 rule); `gamut-avif` (new June 2026, unproven); `cavif` (binary, not a library) | `ravif` directly, default features off, `threading` on | Most established pure-Rust AVIF encoder; `f32` quality gives a finer binary search than `image`'s wrapper. The default `asm` feature is off because it needs the NASM assembler (absent from CI runners); the pure-Rust fallback emits the same AVIF, only slower |
| Target byte size | No maintained crate found for quality-to-size search on AVIF | Own 8-step binary search loop | Not a codec, just a loop over `ravif`; nothing to wrap |

License check: `image` (MIT/Apache-2.0) and `ravif` (BSD-3-Clause, via
`rav1e` BSD-2-Clause) are compatible with the workspace license
(LGPL-3.0-only WITH LGPL-3.0-linking-exception). No GPL/AGPL code.

## API

- `encode_avif(input, max_dim, quality, speed)`: decode, resize to fit
  `max_dim` (aspect kept, never upscaled, Lanczos3), encode at `quality`
  1..=100 and `rav1e` speed 1..=10.
- `encode_avif_to_size(input, max_dim, target_bytes, speed)`: same, with the
  highest quality whose output fits `target_bytes`. Falls back to quality 1
  when nothing fits, and to quality 100 when everything fits.
- `output_info(image)` (plus `AvifImage::dimensions`, `byte_size`, `info`):
  output dimensions and byte size.
- `resized_dimensions(w, h, max_dim)` and `is_avif(bytes)`: helpers.
- Parameter validation happens before decoding; `ravif` panics on
  out-of-range quality/speed, so this crate rejects those with `Error`.

## Platforms

Native and `wasm32-unknown-unknown`. Both dependencies are pure Rust with no
C build step. JPEG XL output is future work (crate description scope).

## License

Same as the workspace: LGPL-3.0-only WITH LGPL-3.0-linking-exception.
