//! Increments: compact deltas of a new payload version against a previous one.
//!
//! This crate is a thin shim over the maintained [`zstd`] compression library.
//! [`diff`] compresses `new` with `old` as a zstd dictionary (the same idea as
//! `zstd --patch-from`), and falls back to storing `new` compressed on its own
//! when the delta is not smaller. A one-byte header records which mode was
//! used, so every delta is self-describing and [`apply`] needs no side
//! information besides the `old` version the delta was built against.
//!
//! Encoding always uses the highest zstd level and embeds a content checksum,
//! so a delta applied against the wrong `old` version fails instead of
//! returning garbage. Decoding needs no upfront capacity: it streams.
//!
//! ```
//! let old = b"the quick brown fox jumps over the lazy dog";
//! let new = b"the quick brown fox jumps over the lazy cat";
//! let delta = cprz_delta::diff(old, new);
//! assert_eq!(cprz_delta::apply(old, &delta).unwrap(), new);
//! ```
//!
//! [`zstd`]: https://crates.io/crates/zstd

use std::io::Read as _;

/// Header byte marking a delta payload: `new` compressed with `old` as the
/// zstd dictionary. The remainder of the payload is that zstd frame.
pub const MODE_DELTA: u8 = 1;

/// Header byte marking a full payload: `new` compressed on its own because
/// the dictionary delta was not smaller. The remainder is that zstd frame.
pub const MODE_FULL: u8 = 2;

/// Failure to apply a delta.
#[derive(Debug)]
pub enum Error {
    /// The delta is empty; at least the one-byte header is required.
    Empty,
    /// The header byte names no known mode.
    UnknownMode(u8),
    /// The zstd frame failed to decode (truncated, corrupt, or built against
    /// a different `old` version than the one supplied).
    Decode(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "delta is empty"),
            Error::UnknownMode(mode) => write!(f, "unknown delta mode: {mode}"),
            Error::Decode(source) => write!(f, "delta failed to decode: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Decode(source) => Some(source),
            _ => None,
        }
    }
}

/// Highest zstd compression level (`ZSTD_MAX_CLEVEL`, stable across zstd
/// releases). Encoding runs once, so it always compresses as far as possible.
const COMPRESSION_LEVEL: i32 = 22;

/// Compress `data` on its own at the highest zstd level, with a content
/// checksum embedded so corruption is detected on decode.
fn compress_full(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = zstd::bulk::Compressor::new(COMPRESSION_LEVEL)?;
    encoder.include_checksum(true)?;
    encoder.compress(data)
}

/// Compress `new` with `old` as a raw-content zstd dictionary (patch-from
/// style) at the highest level, with long-distance matching so edits stay
/// cheap even when `old` is larger than the compression window.
fn compress_delta(old: &[u8], new: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = zstd::bulk::Compressor::with_dictionary(COMPRESSION_LEVEL, old)?;
    encoder.include_checksum(true)?;
    encoder.long_distance_matching(true)?;
    encoder.compress(new)
}

/// Decompress one zstd frame, optionally with `old` as the dictionary.
fn decompress_frame(old: Option<&[u8]>, frame: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    match old {
        Some(dict) => {
            zstd::stream::read::Decoder::with_dictionary(frame, dict)?.read_to_end(&mut out)?;
        }
        None => {
            zstd::stream::read::Decoder::new(frame)?.read_to_end(&mut out)?;
        }
    }
    Ok(out)
}

/// Prepend the mode header to an encoded frame.
fn with_header(mode: u8, frame: Vec<u8>) -> Vec<u8> {
    let mut delta = Vec::with_capacity(1 + frame.len());
    delta.push(mode);
    delta.extend_from_slice(&frame);
    delta
}

/// Build a compact delta of `new` against `old`.
///
/// The delta path compresses `new` with `old` as the zstd dictionary; when
/// that is not strictly smaller than `new` compressed on its own (unrelated
/// data, empty `old`), the standalone encoding is stored instead. The first
/// byte is [`MODE_DELTA`] or [`MODE_FULL`], so [`apply`] can pick the matching
/// decoder.
pub fn diff(old: &[u8], new: &[u8]) -> Vec<u8> {
    // An empty `old` cannot serve as a dictionary; skip straight to the
    // standalone encoding instead of probing a comparison that cannot win.
    let delta = if old.is_empty() {
        None
    } else {
        compress_delta(old, new).ok()
    };
    // A valid level always compresses successfully; there is no smaller
    // fallback left if this fails, so a failure here is a hard error.
    let full = compress_full(new).expect("zstd compression at a valid level cannot fail");
    match delta {
        Some(frame) if frame.len() < full.len() => with_header(MODE_DELTA, frame),
        _ => with_header(MODE_FULL, full),
    }
}

/// Rebuild `new` from `old` and a delta produced by [`diff`].
///
/// Returns an error when the delta is empty, names an unknown mode, or fails
/// to decode. A delta applied against a different base version fails on the
/// embedded checksum whenever the differing bytes change the output; note that
/// a near-identical base whose differences the delta never references can
/// still decode successfully, which is inherent to dictionary deltas.
pub fn apply(old: &[u8], delta: &[u8]) -> Result<Vec<u8>, Error> {
    let (&mode, frame) = delta.split_first().ok_or(Error::Empty)?;
    match mode {
        MODE_DELTA => decompress_frame(Some(old), frame).map_err(Error::Decode),
        MODE_FULL => decompress_frame(None, frame).map_err(Error::Decode),
        other => Err(Error::UnknownMode(other)),
    }
}

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;
