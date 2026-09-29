# cprz

**Shrink any payload as far as possible, then pad it.**

`cprz` is a facade over established compression and media technologies. A
caller says what it has (text, an image, a voice note, a structured record)
and what it needs (for example "must fit one 32 KB message"). `cprz` picks the
best current format, converts the input where needed, shrinks it as far as
possible, and pads the result to a privacy-preserving size class.

It leans strongly towards small payloads: computation is plentiful, bandwidth
and storage are not. Encoding runs once at the highest useful effort;
decoding stays fast.

Status: early development. Interfaces may change.

## Layout

One repository, one version (Cargo workspace). The facade chooses; the
sub-libraries execute. Callers use only the facade.

| Crate | Payload kind | Technologies |
|---|---|---|
| `cprz` | Facade: `shrink`, `expand`, target profiles, the self-describing container | – |
| `cprz-structured` | Records and messages | Schema-driven bit-packing; Cap'n Proto packed where zero-copy or Veilid compatibility matters |
| `cprz-text` | Short and long text | zstd with dictionaries at the highest level; Brotli level 11 for short text without a dictionary |
| `cprz-blob` | Anything else | zstd at the highest level with long-distance matching |
| `cprz-image` | Photos and graphics | Resize to the display size first; AVIF; JPEG XL where supported and for lossless JPEG recompression |
| `cprz-audio` | Voice and sound | Opus; neural speech codecs for voice at very low bitrates |
| `cprz-video` | Clips | AV1 now; AV2 once decoders exist |
| `cprz-delta` | Increments | Delta against a previous version (zstd with the old version as dictionary) |
| `cprz-pad` | Padding | Padmé size classes (at most ~12 % overhead, leaks very little about size) |

Order of operations: **convert → shrink → pad**. Encryption, when used,
happens after padding and outside `cprz`.

## Target profiles

Callers name a target instead of a technology. Profiles carry limits as data,
for example `veilid-message` (32 KB). `cprz` knows these limits but never
depends on a network stack: splitting and framing belong to the transport.

## Principles

1. Always the highest compression level; encoding happens once.
2. Do not send what nobody sees: resize images, choose sample rates to fit speech.
3. Send only what changed (deltas).
4. Dictionaries are trained on public text, never on user plaintext.
5. Every packed payload is self-describing (kind, codec, version), so `expand` needs no side information.
6. No own compression algorithms: wrap established, maintained libraries.

## Platforms

Rust core, built for native targets and `wasm32-unknown-unknown` (browsers).
Media codecs that browsers provide natively (WebCodecs) may be used from the
JavaScript side later; the Rust interface stays the same.

## License

Copyright 2026 Julian Y. Richard Corbet. Licensed under
[LGPL-3.0-only](LICENSES/LGPL-3.0-only.txt)
[WITH LGPL-3.0-linking-exception](LICENSES/LGPL-3.0-linking-exception.txt).
See [LICENSE.md](LICENSE.md).
