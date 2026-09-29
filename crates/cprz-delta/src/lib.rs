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
mod tests {
    use super::*;

    /// Deterministic pseudo-random bytes from a splitmix64 stream, so tests
    /// need no extra dependencies yet still exercise incompressible data.
    fn pseudo_random(len: usize, seed: u64) -> Vec<u8> {
        let mut state = seed;
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            state = state
                .wrapping_add(0x9E37_79B9_7F4A_7C15)
                .wrapping_mul(0xBF58_476D_1CE4_E5B9);
            state ^= state >> 29;
            state = state.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            state ^= state >> 32;
            out.extend_from_slice(&state.to_le_bytes());
        }
        out.truncate(len);
        out
    }

    /// A long, repetitive text payload: small edits must delta to almost
    /// nothing against it.
    fn large_text() -> Vec<u8> {
        let paragraph = "The quick brown fox jumps over the lazy dog. \
            Pack my box with five dozen liquor jugs. \
            How vexingly quick daft zebras jump! ";
        paragraph.repeat(2000).into_bytes()
    }

    #[test]
    fn small_edits_in_large_text_produce_tiny_deltas() {
        let old = large_text();
        let mut new = old.clone();
        new.splice(10_000..10_011, b"swift red wolf".iter().copied());
        new.splice(80_000..80_005, b"owls!".iter().copied());
        new.extend_from_slice(b"One more line at the end.");

        let delta = diff(&old, &new);
        assert_eq!(delta[0], MODE_DELTA);
        assert!(
            delta.len() * 50 < new.len(),
            "delta {} bytes is not tiny against {} bytes",
            delta.len(),
            new.len()
        );
        assert_eq!(apply(&old, &delta).unwrap(), new);
    }

    #[test]
    fn identical_versions_produce_tiny_deltas() {
        let old = large_text();
        let delta = diff(&old, &old);
        assert_eq!(delta[0], MODE_DELTA);
        assert!(delta.len() * 50 < old.len());
        assert_eq!(apply(&old, &delta).unwrap(), old);
    }

    #[test]
    fn unrelated_data_falls_back_to_full() {
        let old = pseudo_random(20_000, 0x1234_5678);
        let new = pseudo_random(20_000, 0xDEAD_BEEF);
        let delta = diff(&old, &new);
        assert_eq!(delta[0], MODE_FULL);
        assert_eq!(apply(&old, &delta).unwrap(), new);
        // Applying a full-mode delta ignores `old` entirely.
        assert_eq!(apply(&pseudo_random(20_000, 1), &delta).unwrap(), new);
    }

    #[test]
    fn binary_round_trips_hold() {
        let mut old: Vec<u8> = (0..=255u8).cycle().take(30_000).collect();
        let new = old.clone();
        assert_eq!(apply(&old, &diff(&old, &new)).unwrap(), new);

        old[29_999] ^= 0xFF;
        assert_eq!(apply(&old, &diff(&old, &old)).unwrap(), old);
    }

    #[test]
    fn empty_inputs_round_trip() {
        assert_eq!(apply(b"", &diff(b"", b"")).unwrap(), b"");
        assert_eq!(apply(b"old", &diff(b"old", b"")).unwrap(), b"");
        let delta = diff(b"", b"brand new");
        assert_eq!(delta[0], MODE_FULL);
        assert_eq!(apply(b"", &delta).unwrap(), b"brand new");
        assert_eq!(apply(b"something else", &delta).unwrap(), b"brand new");
    }

    #[test]
    fn rejects_malformed_deltas() {
        assert!(matches!(apply(b"old", b""), Err(Error::Empty)));
        assert!(matches!(
            apply(b"old", &[0x09]),
            Err(Error::UnknownMode(0x09))
        ));

        // Truncated frame: header names full mode, payload is cut short.
        let mut delta = diff(b"old", b"some new payload");
        assert_eq!(delta[0], MODE_FULL);
        delta.truncate(delta.len() / 2);
        assert!(matches!(apply(b"old", &delta), Err(Error::Decode(_))));

        // Delta applied against an unrelated base version fails: the bytes the
        // delta references come out different, so the checksum does not match.
        let old = large_text();
        let mut new = old.clone();
        new.splice(1000..1010, b"xxxxxxxxxx".iter().copied());
        let delta = diff(&old, &new);
        assert_eq!(delta[0], MODE_DELTA);
        let wrong_old = pseudo_random(old.len(), 0x5EED);
        assert!(matches!(apply(&wrong_old, &delta), Err(Error::Decode(_))));
    }
}
