# cprz-text

Smallest-output lossless compression for UTF-8 text.

Each payload is compressed twice — zstd at the highest level this build
supports (optionally with a caller-supplied dictionary) and Brotli at
quality 11 with a large window — and the smaller frame wins. A one-byte
header records the choice, so decompression needs no side information
besides the dictionary (when one was used). Brotli wins only when strictly
smaller; ties and smaller zstd frames go to zstd.

## Public API

```rust
pub fn compress(text: &str, dictionary: Option<&[u8]>) -> Result<Vec<u8>, Error>;
pub fn decompress(packet: &[u8], dictionary: Option<&[u8]>) -> Result<String, Error>;
pub fn codec_of(packet: &[u8]) -> Result<Codec, Error>;
```

`Codec` is `Zstd` / `ZstdDict` / `Brotli` with `as_byte` / `from_byte`
conversions. `Error` is `Empty`, `UnknownCodec(u8)`, `MissingDictionary`,
`Zstd` / `Brotli` (wrapping the codec I/O failure), and `Utf8` (see
[src/lib.rs](src/lib.rs)).

## Packet format

One header byte followed by the codec frame:

| Header | Codec | Notes |
|---|---|---|
| `0x00` | zstd, no dictionary | Highest level this build supports (`zstd::compression_level_range` end) |
| `0x01` | zstd, with dictionary | Caller must pass the same dictionary to `decompress` |
| `0x02` | Brotli | Quality 11, `lgwin` 24 (16 MiB window) |

Header values are stable and never reused. Dictionaries are trained on public
text, never on user plaintext; the dictionary bytes themselves are not stored
in the packet.

Supplying a dictionary only affects the zstd candidate: an empty dictionary
is treated as absent, and when the Brotli frame is strictly smaller the
packet is a Brotli packet and the dictionary goes unused. `decompress`
ignores the dictionary argument for plain zstd and Brotli frames, and fails
with `MissingDictionary` for a dictionary frame when no (non-empty)
dictionary is supplied. Empty packets fail with `Empty`, unknown header
bytes with `UnknownCodec`, and truncated bodies with the codec-layer error.

## Text handling

`compress` takes `&str` and encodes its exact bytes; no normalization is
performed. `decompress` validates with `String::from_utf8` and fails with
`Utf8` when the decoded bytes are not valid UTF-8, so a non-UTF-8 codec body
decodes through the codec but is refused as text.

## No decoded-size cap

`decompress` takes no size limits and reads each frame to end, so it is not
a bounded entry point for untrusted input: limiting the encoded packet length
alone cannot bound the decompressed output. Decode untrusted packets through
an appropriately bounded decoder path.

The workspace's bounded entry point is
`cprz::portable::decompress(packet, max_encoded_bytes, max_decoded_bytes)`
in [`cprz-blob/src/portable.rs`](../cprz-blob/src/portable.rs), which
enforces caller-supplied encoded and decoded byte limits (see
[the compression contract](../../docs/CONTRACT.md)). It is not a drop-in
decoder for this crate's packets: the portable packet format is codec byte
`0x01` followed by one Brotli stream, different from the header table above,
so a `cprz-text` packet must never be fed to `cprz::portable::decompress`.
When choosing the portable protocol, use `cprz::portable::compress` and
`cprz::portable::decompress` end-to-end; for packets produced by this
crate, use a bounded decoder compatible with this crate's format.

## Relation to the portable path

This crate is the native text utility behind the facade's `Text` payloads
(the facade currently compresses them without a dictionary, see
[`cprz/src/legacy.rs`](../cprz/src/legacy.rs)); it is not the bounded
portable device/transport path. That path is
[`cprz::portable`](../cprz-blob/src/portable.rs), selected with
`default-features = false`: codec byte `0x01`, one Brotli stream at
quality 6 with a 1 MiB window, bounded streaming decoder. Target profiles
such as `veilid-message` (32 KiB) are size-limit data carried by the
facade's `Goal`, not by this crate: `cprz-text` takes no size budget.

The portable CI in this repository does not establish `wasm32` support for
the native text crate; the workflow source is the authority for what CI
runs, see [.github/workflows/ci.yml](../../.github/workflows/ci.yml).

## Historical design survey (September 2026)

The following is the dated September 2026 design survey that selected the
back ends; it is historical. No popularity, download-count, version-currency,
or benchmark claims are made. Both codecs the layer selects between are
established libraries; the selection layer (best of zstd vs Brotli with a
self-describing header) is new, and the crate contains no compression
algorithm of its own.

### zstd back ends

| Candidate | Survey verdict |
|---|---|
| `zstd` 0.14 (bindings to the reference C implementation) | **Chosen.** Exposes the full level range including the highest level, dictionary encode, and streaming decode with dictionaries, as used by [src/lib.rs](src/lib.rs). Declared in [Cargo.toml](Cargo.toml) as `zstd = "0.14"`. |
| `ruzstd` 0.9 | Decoder only; no encoder, so it cannot meet the encode side. |
| `libzstd-rs-sys` (pre-release at survey time) | Decompression and dictionary support only; the encoder was still future work. |
| `structured-zstd` 0.0.x | Pure-Rust alternative; rejected at survey time over the unstable `0.0.x` API signal versus the reference implementation. |
| `zrip` | Levels −7..4 only; cannot do highest-level encoding. |

### Brotli back ends

| Candidate | Survey verdict |
|---|---|
| `brotli` 9 (pure-Rust port of the reference codec) | **Chosen.** Declared in [Cargo.toml](Cargo.toml) as `brotli = "9"`. |
| `brotlic`, `compu-brotli-sys` | C bindings; worse for portability than the pure-Rust port. |
| `burli` | From-scratch rewrite; not established at survey time. |

### Meta-codecs

Benchmark harnesses and content-negotiation layers pick per-payload codecs,
but the survey found no maintained crate offering a "compress with both, keep
the smaller, tag the winner" primitive for embedded payloads.

## License

LGPL-3.0-only WITH LGPL-3.0-linking-exception, like the rest of the
workspace.
