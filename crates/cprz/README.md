# cprz

Facade: shrink any payload as far as possible, then pad it.

Callers say what they have (`Payload`: text, bytes, image, delta of old/new)
and what they need (`Goal`: smallest, fit into N bytes, named target profile
as plain data). `shrink` picks the codec, converts where needed, compresses
at the highest effort, and pads the result with `cprz-pad` when the goal asks
for it. `expand` reverses the process from the bytes alone. Order of
operations is **convert → shrink → pad**; encryption happens after padding
and outside this crate.

This crate chooses; the sub-crates execute. It contains no compression
algorithm and no container machinery of its own beyond a 12-byte header: all
codecs come from `cprz-text`, `cprz-blob`, `cprz-delta`, `cprz-image` and
`cprz-pad` via path dependencies.

## Container format

```
offset  size  field
0       4     magic: b"CPRZ"
4       1     format version (1)
5       1     kind: 0x00 text, 0x01 bytes, 0x02 image, 0x03 delta,
                   0x10 audio, 0x11 video, 0x20 structured (reserved)
6       1     codec id: the sub-crate packet header byte (AVIF marker for images)
7       1     flags: bit 0 = padded with cprz-pad, other bits reserved (zero)
8       4     body length in bytes, big-endian u32
12      N     codec body
```

Text, bytes and delta bodies are the exact sub-crate packets (including their
own one-byte header), so the container codec byte mirrors the body codec byte
and `expand` verifies they agree. Image bodies frame the encode bound first:
4-byte big-endian `max_dim`, one AVIF codec byte, then the AVIF file.

When the goal asks for padding, the whole container is framed with
`cprz-pad`, which prepends an 8-byte length and zero-fills to the Padmé size
class. `expand` sniffs the magic first (a padded buffer starts with a zero
length prefix, never with the magic) and unpads otherwise, so the padding
flag is informational: decoding never needs side information.

## Prior-art survey (required)

Searched crates.io, GitHub and the literature in September 2026, once for the
codecs (nothing new to wrap: every payload kind delegates to its sub-crate,
which records its own survey) and once for the container/framing question.

| Candidate | Result | Verdict |
|---|---|---|
| `postcard` 1.x (James Munns, Apache-2.0, `no_std`, stable wire format since 1.0) | Minimal serde-based binary encoding with COBS framing flavours. | Rejected: a generic serializer adds `serde` derive machinery and has no magic/version/kind header semantics; the header here needs bit-exact sniffing against `cprz-pad` framing, which a generic encoding does not provide. |
| `bincode` 2 (MIT) | Compact serde encoding, no magic, no versioning, config-dependent bytes. | Rejected: same reason as `postcard`, plus no framing story at all. |
| CBOR, RFC 8949 (`minicbor`, `serde_cbor`, MIT/Apache-2.0) | Self-describing binary objects, larger per-message overhead than a fixed header, decoder must implement the full type model. | Rejected: heavier than needed for five fixed fields; pulls a codec framework to encode 12 bytes. |
| MessagePack (`rmp`, MIT) | Compact JSON-like typing, same overhead argument as CBOR. | Rejected: same reason as CBOR. |
| Length-prefix / netstring / COBS framing | The standard answer to "frame a blob on a stream". | Adopted in spirit: the 4-byte body length plus exact-length checks (`Truncated` / `TrailingBytes`) are exactly this, and `cprz-pad` already provides the padded framing layer. Nothing further to wrap. |
| Reusing an AVIF/BMFF, PNG-chunk or zip-like container | Established extensible containers. | Rejected: tens of bytes of box overhead per payload and a parser dependency, for fields that fit in 12 bytes. |

Decision: a fixed 12-byte header defined in this crate, zero new dependencies,
identical bytes on native and `wasm32-unknown-unknown`. The header is framing,
not a compression algorithm or a media container, so it does not duplicate
anything an established crate already covers.

## API

- `shrink(payload, goal) -> Result<Packed, Error>`: convert, compress at the
  highest effort, wrap in the container, pad last when asked.
- `expand(&Packed) -> Result<Payload, Error>`: parse the container (unpading
  first when needed) and decode. Image output holds the AVIF file;
  compression is lossy, so images never round-trip byte-exactly.
- `expand_delta(old, &packed) -> Result<Vec<u8>, Error>`: rebuild the new
  version of a delta container; plain `expand` returns `DeltaNeedsBase`.
- `Goal::smallest[_padded]`, `Goal::fit[_padded](n)`,
  `Goal::for_profile[_padded](p)`: size objective plus padding choice.
- `VEILID_MESSAGE`, `VEILID_DHT_SUBKEY` (both 32 768 bytes),
  `TARGET_PROFILES`, `target_profile(name)`: transport limits as plain data.
  No dependency on any network stack.
- `Kind::{Audio, Video, Structured}`: reserved container values and
  `Error::UnimplementedKind` as extension points for the crates built in
  parallel. This crate does not depend on them.

## Platforms

Native and `wasm32-unknown-unknown`. All codec dependencies come from the
sibling crates, which already prove the wasm build in CI. No network code, no
threads, no OS-specific calls.

## License

Same as the workspace: LGPL-3.0-only WITH LGPL-3.0-linking-exception.
