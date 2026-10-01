//! Exact-byte Brotli packets with a bounded streaming decoder.
//!
//! Wire format: codec byte `0x01`, then one RFC 7932 Brotli stream. Trailing
//! bytes and concatenated streams are rejected. There is no implicit padding.

use brotli::{BrotliDecompressStream, BrotliResult, BrotliState, HeapAlloc};

/// Stable codec discriminator for the portable byte packet.
pub const CODEC: u8 = 0x01;

/// A malformed packet or a caller-supplied resource limit was encountered.
#[derive(Debug)]
pub enum Error {
    /// The packet has no supported codec byte.
    InvalidHeader,
    /// The encoded packet exceeds the caller's limit.
    InputLimit,
    /// The decoded bytes exceed the caller's limit.
    OutputLimit,
    /// The Brotli stream is invalid or incomplete.
    InvalidStream,
    /// Extra bytes follow the complete Brotli stream.
    TrailingBytes,
    /// The upstream encoder returned an I/O failure.
    Encode(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidHeader => "invalid portable codec header",
            Self::InputLimit => "compressed input exceeds limit",
            Self::OutputLimit => "expanded output exceeds limit",
            Self::InvalidStream => "invalid or truncated Brotli stream",
            Self::TrailingBytes => "trailing Brotli input",
            Self::Encode(_) => "Brotli encoding failed",
        })
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(error) => Some(error),
            _ => None,
        }
    }
}

/// Compress arbitrary bytes without conversion, padding, or encryption.
///
/// Quality 6 and a 1 MiB window bound encoder effort for device records. This
/// uses the same maintained pure-Rust Brotli dependency as `cprz-text`.
pub fn compress(mut input: &[u8]) -> Result<Vec<u8>, Error> {
    let params = brotli::enc::BrotliEncoderParams {
        quality: 6,
        lgwin: 20,
        ..Default::default()
    };
    let mut packet = vec![CODEC];
    brotli::BrotliCompress(&mut input, &mut packet, &params).map_err(Error::Encode)?;
    Ok(packet)
}

/// Expand one packet while enforcing encoded and decoded byte limits.
///
/// Input is checked before starting the decoder. Output is drained in at most
/// 4 KiB chunks and checked before appending; a rejected expansion never returns
/// partial plaintext. The upstream strict decoder disables large-window Brotli
/// extensions (RFC 7932 permits at most a 16 MiB history window). These limits
/// bound packet/output bytes, not total allocator overhead or execution time.
pub fn decompress(
    packet: &[u8],
    max_encoded_bytes: usize,
    max_decoded_bytes: usize,
) -> Result<Vec<u8>, Error> {
    if packet.len() > max_encoded_bytes {
        return Err(Error::InputLimit);
    }
    if packet.first() != Some(&CODEC) {
        return Err(Error::InvalidHeader);
    }
    let input = &packet[1..];
    let mut state = BrotliState::new_strict(
        HeapAlloc::<u8>::default(),
        HeapAlloc::<u32>::default(),
        HeapAlloc::<brotli::HuffmanCode>::default(),
    );
    let mut available_in = input.len();
    let mut input_offset = 0;
    let mut total_out = 0;
    let mut output = Vec::new();
    loop {
        let mut chunk = [0; 4096];
        let mut available_out = chunk.len().min(
            max_decoded_bytes
                .saturating_sub(output.len())
                .saturating_add(1),
        );
        let mut output_offset = 0;
        let result = BrotliDecompressStream(
            &mut available_in,
            &mut input_offset,
            input,
            &mut available_out,
            &mut output_offset,
            &mut chunk,
            &mut total_out,
            &mut state,
        );
        if output_offset > max_decoded_bytes - output.len() {
            return Err(Error::OutputLimit);
        }
        output.extend_from_slice(&chunk[..output_offset]);
        match result {
            BrotliResult::ResultSuccess => {
                if available_in != 0 {
                    return Err(Error::TrailingBytes);
                }
                return Ok(output);
            }
            BrotliResult::NeedsMoreOutput => {}
            BrotliResult::NeedsMoreInput | BrotliResult::ResultFailure => {
                return Err(Error::InvalidStream);
            }
        }
    }
}
