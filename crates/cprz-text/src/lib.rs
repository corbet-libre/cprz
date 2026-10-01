//! Smallest-output lossless compression for UTF-8 text.
//!
//! Each payload is compressed twice: once with zstd at the highest available
//! level (optionally with a caller-supplied dictionary) and once with Brotli
//! at quality 11 with a large window. The smaller result wins and a one-byte
//! header records which codec was used, so [`decompress`] needs no side
//! information besides the dictionary (when one was used).
//!
//! On ties zstd wins because it decodes faster.
//!
//! # Example
//!
//! ```
//! let packet = cprz_text::compress("Hello, world!", None)?;
//! let back = cprz_text::decompress(&packet, None)?;
//! assert_eq!(back, "Hello, world!");
//! # Ok::<(), cprz_text::Error>(())
//! ```

use std::io::{Read, Write};

/// Header byte marking a plain zstd frame.
const HEADER_ZSTD: u8 = 0x00;
/// Header byte marking a zstd frame compressed with a dictionary.
const HEADER_ZSTD_DICT: u8 = 0x01;
/// Header byte marking a Brotli frame.
const HEADER_BROTLI: u8 = 0x02;

/// Brotli quality: the maximum, favouring the smallest output.
const BROTLI_QUALITY: u32 = 11;
/// Brotli window bits: the maximum (16 MiB window) for long-range matches.
const BROTLI_LGWIN: u32 = 24;
/// I/O buffer size for the Brotli reader/writer wrappers.
const BROTLI_BUFFER_SIZE: usize = 4096;

/// The codec a packet was compressed with.
///
/// Matches the one-byte header documented in the crate README. Numeric values
/// are stable: never reuse a value for a different codec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Codec {
    /// zstd at the highest level, without a dictionary.
    Zstd,
    /// zstd at the highest level, with the caller's dictionary.
    ZstdDict,
    /// Brotli at quality 11 with a large window.
    Brotli,
}

impl Codec {
    /// Return the header byte identifying this codec.
    #[must_use]
    pub fn as_byte(self) -> u8 {
        match self {
            Codec::Zstd => HEADER_ZSTD,
            Codec::ZstdDict => HEADER_ZSTD_DICT,
            Codec::Brotli => HEADER_BROTLI,
        }
    }

    /// Parse a header byte into a codec.
    #[must_use]
    pub fn from_byte(byte: u8) -> Option<Codec> {
        match byte {
            HEADER_ZSTD => Some(Codec::Zstd),
            HEADER_ZSTD_DICT => Some(Codec::ZstdDict),
            HEADER_BROTLI => Some(Codec::Brotli),
            _ => None,
        }
    }
}

/// Errors from compressing or decompressing text.
#[derive(Debug)]
pub enum Error {
    /// The packet is empty; at least the one-byte header is required.
    Empty,
    /// The header byte names no known codec.
    UnknownCodec(u8),
    /// The packet was compressed with a dictionary but none was supplied.
    MissingDictionary,
    /// The zstd layer failed.
    Zstd(std::io::Error),
    /// The Brotli layer failed.
    Brotli(std::io::Error),
    /// The decompressed bytes are not valid UTF-8.
    Utf8(std::string::FromUtf8Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "empty packet"),
            Error::UnknownCodec(byte) => write!(f, "unknown codec header {byte:#04X}"),
            Error::MissingDictionary => {
                write!(f, "packet needs the dictionary it was compressed with")
            }
            Error::Zstd(source) => write!(f, "zstd error: {source}"),
            Error::Brotli(source) => write!(f, "brotli error: {source}"),
            Error::Utf8(source) => write!(f, "decompressed bytes are not UTF-8: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Zstd(source) | Error::Brotli(source) => Some(source),
            Error::Utf8(source) => Some(source),
            _ => None,
        }
    }
}

/// Highest zstd level this build supports (level 22 for the reference library).
fn zstd_level() -> i32 {
    *zstd::compression_level_range().end()
}

fn encode_zstd(input: &[u8], dictionary: Option<&[u8]>) -> Result<Vec<u8>, Error> {
    let level = zstd_level();
    let mut encoder = match dictionary {
        Some(dict) => zstd::bulk::Compressor::with_dictionary(level, dict).map_err(Error::Zstd)?,
        None => zstd::bulk::Compressor::new(level).map_err(Error::Zstd)?,
    };
    encoder.compress(input).map_err(Error::Zstd)
}

fn decode_zstd(input: &[u8], dictionary: Option<&[u8]>) -> Result<Vec<u8>, Error> {
    let mut output = Vec::new();
    match dictionary {
        Some(dict) => {
            let mut decoder = zstd::Decoder::with_dictionary(input, dict).map_err(Error::Zstd)?;
            decoder.read_to_end(&mut output).map_err(Error::Zstd)?;
        }
        None => {
            let mut decoder = zstd::Decoder::new(input).map_err(Error::Zstd)?;
            decoder.read_to_end(&mut output).map_err(Error::Zstd)?;
        }
    }
    Ok(output)
}

fn encode_brotli(input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut writer =
        brotli::CompressorWriter::new(Vec::new(), BROTLI_BUFFER_SIZE, BROTLI_QUALITY, BROTLI_LGWIN);
    writer.write_all(input).map_err(Error::Brotli)?;
    // Into_inner finishes the stream; the frame is complete afterwards.
    Ok(writer.into_inner())
}

fn decode_brotli(input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut reader = brotli::Decompressor::new(input, BROTLI_BUFFER_SIZE);
    let mut output = Vec::new();
    reader.read_to_end(&mut output).map_err(Error::Brotli)?;
    Ok(output)
}

/// Compress UTF-8 text to the smallest output of zstd and Brotli.
///
/// `dictionary` is an optional caller-supplied zstd dictionary (trained on
/// public text, never on user plaintext). An empty dictionary is treated as
/// absent. The returned packet is the one-byte codec header followed by the
/// winning frame; ties go to zstd.
pub fn compress(text: &str, dictionary: Option<&[u8]>) -> Result<Vec<u8>, Error> {
    let input = text.as_bytes();
    let dictionary = dictionary.filter(|dict| !dict.is_empty());
    let zstd_frame = encode_zstd(input, dictionary)?;
    let brotli_frame = encode_brotli(input)?;
    let (codec, frame) = if brotli_frame.len() < zstd_frame.len() {
        (Codec::Brotli, brotli_frame)
    } else {
        (
            if dictionary.is_some() {
                Codec::ZstdDict
            } else {
                Codec::Zstd
            },
            zstd_frame,
        )
    };
    let mut packet = Vec::with_capacity(1 + frame.len());
    packet.push(codec.as_byte());
    packet.extend_from_slice(&frame);
    Ok(packet)
}

/// Report which codec a packet uses, reading only its header byte.
pub fn codec_of(packet: &[u8]) -> Result<Codec, Error> {
    let header = packet.first().ok_or(Error::Empty)?;
    Codec::from_byte(*header).ok_or(Error::UnknownCodec(*header))
}

/// Decompress a packet from [`compress`] back to its original text.
///
/// Pass the same `dictionary` that compression used when the header records a
/// dictionary frame; it is ignored for plain zstd and Brotli frames.
pub fn decompress(packet: &[u8], dictionary: Option<&[u8]>) -> Result<String, Error> {
    let codec = codec_of(packet)?;
    let body = &packet[1..];
    let bytes = match codec {
        Codec::Zstd => decode_zstd(body, None)?,
        Codec::ZstdDict => {
            let dict = dictionary
                .filter(|dict| !dict.is_empty())
                .ok_or(Error::MissingDictionary)?;
            decode_zstd(body, Some(dict))?
        }
        Codec::Brotli => decode_brotli(body)?,
    };
    String::from_utf8(bytes).map_err(Error::Utf8)
}

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;
