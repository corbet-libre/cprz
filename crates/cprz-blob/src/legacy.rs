//! Smallest-output lossless compression for arbitrary bytes.
//!
//! Payloads are compressed with zstd at the highest available level with
//! long-distance matching enabled, which finds repeats far apart in large
//! inputs (backups, bundles, dumps). A one-byte header identifies the codec,
//! so [`decompress`] needs no side information.
//!
//! # Example
//!
//! ```
//! let input = b"some opaque bytes".repeat(100);
//! let packet = cprz_blob::compress(&input)?;
//! let back = cprz_blob::decompress(&packet)?;
//! assert_eq!(back, input);
//! # Ok::<(), cprz_blob::Error>(())
//! ```

use std::io::Read;

/// Header byte marking a zstd frame with long-distance matching.
const HEADER_ZSTD: u8 = 0x00;

/// The codec a packet was compressed with.
///
/// Matches the one-byte header documented in the crate README. Numeric values
/// are stable: never reuse a value for a different codec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Codec {
    /// zstd at the highest level with long-distance matching.
    Zstd,
}

impl Codec {
    /// Return the header byte identifying this codec.
    #[must_use]
    pub fn as_byte(self) -> u8 {
        match self {
            Codec::Zstd => HEADER_ZSTD,
        }
    }

    /// Parse a header byte into a codec.
    #[must_use]
    pub fn from_byte(byte: u8) -> Option<Codec> {
        match byte {
            HEADER_ZSTD => Some(Codec::Zstd),
            _ => None,
        }
    }
}

/// Errors from compressing or decompressing blobs.
#[derive(Debug)]
pub enum Error {
    /// The packet is empty; at least the one-byte header is required.
    Empty,
    /// The header byte names no known codec.
    UnknownCodec(u8),
    /// The zstd layer failed.
    Zstd(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "empty packet"),
            Error::UnknownCodec(byte) => write!(f, "unknown codec header {byte:#04X}"),
            Error::Zstd(source) => write!(f, "zstd error: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Zstd(source) => Some(source),
            _ => None,
        }
    }
}

/// Compress arbitrary bytes.
///
/// Uses zstd at the highest level this build supports (level 22 for the
/// reference library) with long-distance matching enabled. The returned
/// packet is the one-byte codec header followed by the frame.
pub fn compress(input: &[u8]) -> Result<Vec<u8>, Error> {
    let level = *zstd::compression_level_range().end();
    let mut encoder = zstd::bulk::Compressor::new(level).map_err(Error::Zstd)?;
    encoder.long_distance_matching(true).map_err(Error::Zstd)?;
    let frame = encoder.compress(input).map_err(Error::Zstd)?;
    let mut packet = Vec::with_capacity(1 + frame.len());
    packet.push(Codec::Zstd.as_byte());
    packet.extend_from_slice(&frame);
    Ok(packet)
}

/// Report which codec a packet uses, reading only its header byte.
pub fn codec_of(packet: &[u8]) -> Result<Codec, Error> {
    let header = packet.first().ok_or(Error::Empty)?;
    Codec::from_byte(*header).ok_or(Error::UnknownCodec(*header))
}

/// Decompress a packet from [`compress`] back to its original bytes.
///
/// Long-distance matching is an encoder-side setting stored in the frame, so
/// decoding needs no extra configuration.
pub fn decompress(packet: &[u8]) -> Result<Vec<u8>, Error> {
    match codec_of(packet)? {
        Codec::Zstd => {
            let mut decoder = zstd::Decoder::new(&packet[1..]).map_err(Error::Zstd)?;
            let mut output = Vec::new();
            decoder.read_to_end(&mut output).map_err(Error::Zstd)?;
            Ok(output)
        }
    }
}

#[cfg(test)]
#[path = "tests/legacy.rs"]
mod tests;
