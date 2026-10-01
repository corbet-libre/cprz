# Compression contract

`cprz::portable::compress(input: &[u8]) -> Result<Vec<u8>, Error>` preserves every
input byte, including empty and non-UTF8 input. It produces codec byte 0x01 and
one Brotli stream. The portable facade without the default native codecs is selected with
`cprz = { git = "https://github.com/corbet-foss/cprz", branch = "main", default-features = false }`.

`cprz::portable::decompress(packet, max_encoded_bytes, max_decoded_bytes)` checks
input length before codec work, enforces output size during streaming expansion,
and rejects truncated/invalid streams, unsupported headers and all trailing
bytes. Limits include the one-byte packet discriminator. No partial plaintext
is returned on error. No claim is made to authenticate compressed data.

Callers authenticate/decrypt VLD0, remove Padme, and only then decode with their
record-size limits. Compression never pads or encrypts implicitly. The legacy
native general-purpose facade retains its original wire format and behavior.
No registry publication. No domain logging, network, identity or key storage.
