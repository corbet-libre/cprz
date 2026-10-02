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

## Scope

### Purpose

`cprz` shrinks every payload as far as possible with established technologies and pads it to a privacy-preserving size class before the caller encrypts.

### Owns

Home `cmsg` → `cbrd` → `cprz`, reused by Profile and by vault record packing. The facade `shrink`/`expand`, target profiles as data (for example `veilid-message` 32 KB), a self-describing container, and a bounded portable path for device storage and transport; the facade chooses and the sublibraries below execute.

| Crate | Job |
|---|---|
| `cprz-structured` | Records and messages: schema bit-packing, Cap'n Proto where needed |
| `cprz-text` | Text: zstd with public dictionaries, Brotli 11 |
| `cprz-blob` | Other bytes: zstd long-distance |
| `cprz-image` | Resize then AVIF / JPEG XL |
| `cprz-audio` | Opus; neural speech codecs later |
| `cprz-video` | AV1 now, AV2 later |
| `cprz-delta` | Increments against a previous version |
| `cprz-pad` | Padmé size classes |

### Never

Does not encrypt, frame streams, or depend on a network stack. Does not train dictionaries on member plaintext; dictionaries use public data only. Does not implement its own compression algorithm; it wraps maintained libraries. Profile pictures remain outside the launch product. `cfrm` is not a compression service.

### States

Stateless: input becomes encoded and padded output or is refused; decode yields validated bytes or is refused; delta decompression takes the base version as explicit input.

### Test obligations

Codec round trips with identical vectors on native and WebAssembly targets. Bounded expansion and work; truncated, concatenated, trailing and unknown-codec input is refused. Deterministic public dictionaries; signing and encryption round trips prove signed bytes unchanged. Order is compress, then pad, then encrypt. Rust core on current stable; full line and branch coverage with real round trips and no mocks of own logic, including injected delay, duplication, loss, cancellation, clock regression, corruption and storage conflicts; bounded bytes, parsing, queues and work with errors omitting identifiers, plaintext and secrets; independent keys, transport identities, record locators, sessions and stores per community with nothing exported across communities.

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

## Continuous verification

Dependency updates follow main and are tested against one CI-resolved lockfile.
Line and branch coverage target 100%; failures remain blocking. See
[the coverage contract](docs/COVERAGE.md) for measurement and exclusions.

## Bounded portable records

Device storage and transport use `cprz` with `default-features = false` and
`cprz::portable::{compress, decompress}`. The decoder requires both encoded and
decoded byte limits; oversize, truncated, concatenated and trailing data fail.
It preserves exact bytes without image conversion, padding or encryption.
Pipeline: compress, then Padme, then VLD0 encryption; reverse on receipt.

The portable wire packet is codec byte `0x01` plus one RFC 7932 Brotli stream.
It is deliberately a byte packet, not the legacy general CPRZ container. The
maintained pure-Rust `brotli` dependency already used by cprz-text was selected
for native/wasm portability. Existing zstd and image codecs remain available with
default features; their legacy decoders are not bounded storage entry points.
`brotlic`/`brotli2` require C FFI; zstd is retained for native workloads. No codec
or cryptographic primitive is implemented here. Upstream API references:
[Brotli streaming decoder](https://docs.rs/brotli/latest/brotli/fn.BrotliDecompressStream.html)
and [strict decoder state](https://docs.rs/brotli-decompressor/latest/brotli_decompressor/struct.BrotliState.html).

The strict upstream decoder disables large-window extensions. Output grows only
after each at-most-4-KiB chunk fits the caller's output limit. The RFC permits up
to 16 MiB decoder history; limits are not a whole-process memory/CPU quota.
Quality 6 with a 1 MiB encoder window bounds routine device effort. CI executes
identical hostile-input and round-trip vectors natively and in wasm. The legacy
full workspace is native-only until its media/native-codec dependencies are
ported; the portable facade explicitly excludes them.

### Opus pre-skip boundary

The audio decoder discards the complete requested prefix, including a prefix
that spans all decoded samples. It rejects pre-skip beyond the available audio
before rounding to the output rate. This corrects a real one-packet regression
that previously returned all 320 decoded samples when 960 samples at 48 kHz meant
to discard them all at 16 kHz. Real Opus/Ogg vectors cover zero, partial, complete
and excessive prefixes. This follows [RFC 7845 sections 4.2 and 4.5](https://www.rfc-editor.org/rfc/rfc7845#section-4.2); it is not a claim of full Ogg conformance.
