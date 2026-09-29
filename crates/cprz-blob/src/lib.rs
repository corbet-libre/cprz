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
mod tests {
    use super::*;

    /// 256 KiB of periodic bytes, like a memory image with structure.
    fn repetitive_blob() -> Vec<u8> {
        (0..262_144).map(|i| (i % 251) as u8).collect()
    }

    /// 640 KiB with a 64 KiB block repeated half a megabyte apart, so only
    /// long-distance matching can link the copies.
    fn distant_repeat_blob() -> Vec<u8> {
        let block: Vec<u8> = (0..65_536).map(|i| (i % 251) as u8).collect();
        let filler: Vec<u8> = (0..524_288).map(|i| ((i * 31 + 7) % 251) as u8).collect();
        let mut data = Vec::with_capacity(block.len() * 2 + filler.len());
        data.extend_from_slice(&block);
        data.extend_from_slice(&filler);
        data.extend_from_slice(&block);
        data
    }

    /// Deterministic pseudo-random bytes that barely compress.
    fn incompressible_blob() -> Vec<u8> {
        let mut state: u32 = 0x1234_5678;
        (0..4096)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state & 0xff) as u8
            })
            .collect()
    }

    #[test]
    fn header_value_is_stable() {
        assert_eq!(Codec::Zstd.as_byte(), 0x00);
        assert_eq!(Codec::from_byte(0x09), None);
        assert_eq!(codec_of(&[0x00]).unwrap(), Codec::Zstd);
    }

    #[test]
    fn repetitive_blob_round_trip_with_ratio() {
        let input = repetitive_blob();
        let packet = compress(&input).unwrap();
        assert_eq!(decompress(&packet).unwrap(), input);
        // Periodic data must shrink to well under half its size.
        assert!(
            packet.len() < input.len() / 2,
            "packet {} vs input {}",
            packet.len(),
            input.len()
        );
    }

    #[test]
    fn distant_repeat_blob_round_trip_with_ratio() {
        let input = distant_repeat_blob();
        let packet = compress(&input).unwrap();
        assert_eq!(decompress(&packet).unwrap(), input);
        assert!(
            packet.len() < input.len() / 2,
            "packet {} vs input {}",
            packet.len(),
            input.len()
        );
    }

    #[test]
    fn incompressible_blob_round_trip() {
        let input = incompressible_blob();
        let packet = compress(&input).unwrap();
        assert_eq!(decompress(&packet).unwrap(), input);
    }

    #[test]
    fn empty_input_round_trip() {
        let packet = compress(&[]).unwrap();
        assert_eq!(decompress(&packet).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn compression_is_deterministic() {
        let input = repetitive_blob();
        assert_eq!(compress(&input).unwrap(), compress(&input).unwrap());
    }

    #[test]
    fn malformed_packets_are_errors() {
        assert!(matches!(decompress(&[]), Err(Error::Empty)));
        assert!(matches!(codec_of(&[]), Err(Error::Empty)));
        assert!(matches!(
            decompress(&[0x09, 0x00]),
            Err(Error::UnknownCodec(0x09))
        ));
        // Truncated body behind a valid header.
        let packet = compress(&repetitive_blob()).unwrap();
        let truncated = &packet[..packet.len() / 2];
        assert!(decompress(truncated).is_err());
    }
}
