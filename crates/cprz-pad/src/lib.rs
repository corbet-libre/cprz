//! Privacy-preserving padding using the Padmé size classes.
//!
//! This crate implements the Padmé padding scheme from Nikitin et al.,
//! "Padded Uniform Random Blobs (PURBs)" (2019, Section 4): the padded length
//! of a message is rounded up to a representative of its floating-point
//! range, so all messages that share the same exponent and mantissa width
//! look identical on the wire. Padmé leaks only `O(log log M)` bits about the
//! length (with `M` the maximum message length), the same order as padding to
//! a power of two, but its overhead is at most about 12% and shrinks as
//! messages grow.
//!
//! Two modes are provided:
//!
//! * Default: Padmé size classes (see [`padded_len`]). Use this unless there
//!   is a reason not to.
//! * [`fixed`]: power-of-two size classes for very sensitive data. Coarser
//!   (up to ~100% overhead) but fewer distinct lengths.
//!
//! # Framing
//!
//! [`pad`] prepends an 8-byte big-endian encoding of the data length (see
//! [`HEADER_LEN`]), appends the data, and fills the remainder of the size
//! class with zero bytes. [`unpad`] reads that prefix back, so `unpad` works
//! for both modes and recovers the exact input: `unpad(pad(x)) == x`.
//! Non-zero bytes in the padding area and length prefixes that do not fit
//! the buffer are rejected with [`Error`].
//!
//! The 8-byte header dominates the relative overhead of tiny messages; the
//! ~12% bound holds once the header is amortised (see tests).
//!
//! See `README.md` for why the size-class rounding is implemented here
//! instead of reused from an existing crate.

/// Length of the unambiguous length prefix in bytes.
///
/// Every padded buffer starts with the original data length encoded as an
/// 8-byte big-endian `u64`, followed by the data itself and zero padding.
pub const HEADER_LEN: usize = 8;

/// Largest framed length the fixed power-of-two mode addresses exactly.
///
/// Inputs whose framed length (`len + HEADER_LEN`) exceeds this saturate to
/// [`usize::MAX`] in [`fixed::padded_len`]; such buffers cannot be allocated
/// in practice, and [`fixed::pad`] would fail while trying.
pub const FIXED_MAX_FRAMED: u64 = 1 << 63;

/// Error returned by [`unpad`] (and [`fixed::unpad`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The input is shorter than [`HEADER_LEN`], so it holds no length prefix.
    Empty,
    /// The declared data length does not fit the buffer.
    InvalidLength {
        /// Data length read from the 8-byte prefix.
        declared: u64,
        /// Bytes available after the prefix.
        available: usize,
    },
    /// A byte in the padding area (after the declared data) is not zero.
    NonZeroPadding,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "buffer is shorter than the length prefix"),
            Self::InvalidLength {
                declared,
                available,
            } => write!(
                f,
                "declared length {declared} exceeds available {available} bytes"
            ),
            Self::NonZeroPadding => write!(f, "padding area holds non-zero bytes"),
        }
    }
}

impl std::error::Error for Error {}

/// Round `len` (a total, framed length) up to its Padmé size class.
///
/// Pure size-class oracle over `u64`, independent of the pointer width, so
/// native targets and `wasm32-unknown-unknown` agree exactly. Returns `len`
/// itself for `len < 2`; saturates at `u64::MAX` instead of overflowing.
fn padme_padded_len(len: u64) -> u64 {
    if len < 2 {
        return len;
    }
    // Exponent of the floating-point range holding `len`.
    let e = 63 - len.leading_zeros();
    // Bit length of the exponent: the mantissa width that stays visible.
    let s = 64 - u64::from(e).leading_zeros();
    // Low `z` bits are rounded away.
    let z = e - s;
    let mask = (1u64 << z) - 1;
    len.saturating_add(mask) & !mask
}

/// Padded buffer length for `len` bytes of data in the default Padmé mode.
///
/// Covers the [`HEADER_LEN`]-byte prefix plus the data, rounded up to the
/// Padmé class. Always `>= len + HEADER_LEN`; saturates to [`usize::MAX`]
/// for non-allocatable lengths near [`usize::MAX`].
#[must_use]
pub fn padded_len(len: usize) -> usize {
    let framed = (len as u64).saturating_add(HEADER_LEN as u64);
    let out = padme_padded_len(framed);
    // The oracle saturates instead of overflowing; report saturation so the
    // result never drops below the framed length.
    let out = if out < framed { u64::MAX } else { out };
    usize::try_from(out).unwrap_or(usize::MAX)
}

/// Number of overhead bytes [`pad`] adds for `len` bytes of data.
///
/// This is `padded_len(len) - len`: the length prefix plus zero padding.
#[must_use]
pub fn padding_len(len: usize) -> usize {
    padded_len(len).saturating_sub(len)
}

/// Pad `data` to its Padmé size class (see [`padded_len`]).
///
/// The output starts with the data length as 8-byte big-endian, followed by
/// the data and zero bytes. Its length equals `padded_len(data.len())`.
#[must_use]
pub fn pad(data: &[u8]) -> Vec<u8> {
    frame(data, padded_len(data.len()))
}

/// Recover the exact input of [`pad`] (or [`fixed::pad`]).
///
/// # Errors
///
/// Returns [`Error::Empty`] when the input holds no length prefix,
/// [`Error::InvalidLength`] when the declared length does not fit, and
/// [`Error::NonZeroPadding`] when padding bytes are not zero.
pub fn unpad(padded: &[u8]) -> Result<Vec<u8>, Error> {
    if padded.len() < HEADER_LEN {
        return Err(Error::Empty);
    }
    let mut prefix = [0u8; HEADER_LEN];
    prefix.copy_from_slice(&padded[..HEADER_LEN]);
    let declared = u64::from_be_bytes(prefix);
    let available = padded.len() - HEADER_LEN;
    let data_len = usize::try_from(declared).map_err(|_| Error::InvalidLength {
        declared,
        available,
    })?;
    if data_len > available {
        return Err(Error::InvalidLength {
            declared,
            available,
        });
    }
    if padded[HEADER_LEN + data_len..]
        .iter()
        .any(|&byte| byte != 0)
    {
        return Err(Error::NonZeroPadding);
    }
    Ok(padded[HEADER_LEN..HEADER_LEN + data_len].to_vec())
}

/// Build a zero-padded buffer holding the length prefix plus `data`.
fn frame(data: &[u8], total: usize) -> Vec<u8> {
    let mut out = vec![0u8; total];
    out[..HEADER_LEN].copy_from_slice(&(data.len() as u64).to_be_bytes());
    out[HEADER_LEN..HEADER_LEN + data.len()].copy_from_slice(data);
    out
}

/// Fixed power-of-two size classes for very sensitive data.
///
/// Where the default Padmé mode trades some length resolution for low
/// overhead, this mode rounds the framed length up to the next power of two,
/// so only `O(log M)` distinct output lengths exist. Overhead can approach
/// 100% just above a class boundary. Buffers use the same framing as [`pad`],
/// so [`unpad`] decodes both modes.
pub mod fixed {
    use super::{Error, HEADER_LEN, frame};
    use crate::FIXED_MAX_FRAMED;

    /// Padded buffer length for `len` bytes of data in fixed mode.
    ///
    /// Smallest power of two that holds `len + HEADER_LEN`. Saturates to
    /// [`usize::MAX`] for non-allocatable lengths.
    #[must_use]
    pub fn padded_len(len: usize) -> usize {
        let framed = (len as u64).saturating_add(HEADER_LEN as u64);
        if framed > FIXED_MAX_FRAMED {
            return usize::MAX;
        }
        let class = framed.checked_next_power_of_two().unwrap_or(u64::MAX);
        usize::try_from(class).unwrap_or(usize::MAX)
    }

    /// Number of overhead bytes [`pad`](self::pad) adds for `len` bytes.
    #[must_use]
    pub fn padding_len(len: usize) -> usize {
        padded_len(len).saturating_sub(len)
    }

    /// Pad `data` up to the next power-of-two size class.
    ///
    /// Same framing as [`pad`](crate::pad); its length equals
    /// [`padded_len`](self::padded_len) of the input length.
    #[must_use]
    pub fn pad(data: &[u8]) -> Vec<u8> {
        frame(data, padded_len(data.len()))
    }

    /// Recover the exact input of [`pad`](self::pad) (or [`pad`](crate::pad)).
    ///
    /// # Errors
    ///
    /// Same conditions as [`unpad`](crate::unpad): [`Error::Empty`],
    /// [`Error::InvalidLength`], or [`Error::NonZeroPadding`].
    pub fn unpad(padded: &[u8]) -> Result<Vec<u8>, Error> {
        super::unpad(padded)
    }
}

#[cfg(test)]
mod tests {
    use super::{fixed, pad, padded_len, unpad};

    /// Deterministic pseudo-random filler so tests never rely on zero data.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| (i.wrapping_mul(31).wrapping_add(len) & 0xff) as u8)
            .collect()
    }

    /// Lengths 0..=1024 exhaustive, then a stride up to 100000.
    fn sample_lengths() -> Vec<usize> {
        let mut out: Vec<usize> = (0..=1024).collect();
        let mut n = 1025;
        while n <= 100_000 {
            out.push(n);
            n += 13;
        }
        out.push(99_999);
        out.push(100_000);
        out
    }

    #[test]
    fn oracle_matches_reference_vectors() {
        // Checked against the PURBs Section 4 construction: each expected
        // value is the Padmé oracle applied to framed = data_len + 8.
        let vectors = [
            (0_usize, 8_usize),
            (1, 10),
            (2, 10),
            (3, 12),
            (8, 16),
            (9, 18),
            (15, 24),
            (16, 24),
            (17, 26),
            (25, 36),
            (33, 44),
            (92, 104),
            (121, 144),
            (1000, 1024),
            (100_000, 100_352),
        ];
        for (len, expected) in vectors {
            assert_eq!(padded_len(len), expected, "len {len}");
        }
    }

    #[test]
    fn oracle_overhead_stays_within_bound() {
        // Pure oracle (without the framing header) over every length:
        // output covers the input and overhead stays at about 12%.
        for len in 1..=100_000_usize {
            let out = super::padme_padded_len(len as u64) as usize;
            assert!(out >= len, "len {len}");
            assert!(
                (out - len) * 100 <= len * 12,
                "len {len}: {out} exceeds 12% overhead"
            );
        }
        assert_eq!(super::padme_padded_len(0), 0);
    }

    #[test]
    fn padded_output_is_a_class_member() {
        // Padding an already padded length changes nothing: outputs are
        // fixed points of the oracle (idempotent size classes).
        for len in sample_lengths() {
            let out = padded_len(len) as u64;
            assert_eq!(super::padme_padded_len(out), out, "len {len}");
        }
    }

    #[test]
    fn roundtrip_sampled_lengths() {
        for len in sample_lengths() {
            let data = pattern(len);
            let padded = pad(&data);
            assert_eq!(padded.len(), padded_len(len), "len {len}");
            assert_eq!(unpad(&padded), Ok(data), "len {len}");
        }
    }

    #[test]
    fn framed_overhead_stays_within_bound() {
        // With the 8-byte header amortised (len >= 256) the total overhead
        // of `pad` stays within about 12% up to 100000.
        for len in sample_lengths().into_iter().filter(|&n| n >= 256) {
            let out = padded_len(len);
            assert!(out >= len + super::HEADER_LEN, "len {len}");
            assert!(
                (out - len) * 100 <= len * 12,
                "len {len}: {out} exceeds 12% overhead"
            );
        }
    }

    #[test]
    fn unpad_rejects_malformed_input() {
        assert_eq!(unpad(&[]), Err(super::Error::Empty));
        assert_eq!(unpad(&[0u8; 7]), Err(super::Error::Empty));

        // Declared length larger than the buffer holds.
        let mut oversized = [0u8; 8];
        oversized.copy_from_slice(&9u64.to_be_bytes());
        assert_eq!(
            unpad(&oversized),
            Err(super::Error::InvalidLength {
                declared: 9,
                available: 0,
            })
        );

        // Truncated into the data area.
        let padded = pad(b"hello");
        assert_eq!(
            unpad(&padded[..10]),
            Err(super::Error::InvalidLength {
                declared: 5,
                available: 2,
            })
        );

        // Corrupted padding byte.
        let mut tampered = pad(b"hello");
        let last = tampered.len() - 1;
        tampered[last] = 0xff;
        assert_eq!(unpad(&tampered), Err(super::Error::NonZeroPadding));
    }

    #[test]
    fn empty_input_roundtrips() {
        let padded = pad(&[]);
        assert_eq!(padded.len(), padded_len(0));
        assert_eq!(unpad(&padded), Ok(Vec::new()));
    }

    #[test]
    fn fixed_classes_roundtrip() {
        let vectors = [(0_usize, 8_usize), (1, 16), (8, 16), (9, 32), (100, 128)];
        for (len, expected) in vectors {
            assert_eq!(fixed::padded_len(len), expected, "len {len}");
        }
        for len in sample_lengths() {
            let data = pattern(len);
            let padded = fixed::pad(&data);
            let framed = len + super::HEADER_LEN;
            assert_eq!(padded.len(), fixed::padded_len(len), "len {len}");
            assert!(padded.len().is_power_of_two(), "len {len}");
            assert!(padded.len() >= framed, "len {len}");
            assert!(padded.len() < 2 * framed, "len {len}");
            assert_eq!(unpad(&padded), Ok(data.clone()), "len {len}");
            assert_eq!(fixed::unpad(&padded), Ok(data), "len {len}");
        }
    }

    #[test]
    fn huge_lengths_saturate_instead_of_wrapping() {
        assert_eq!(padded_len(usize::MAX), usize::MAX);
        assert_eq!(fixed::padded_len(usize::MAX), usize::MAX);
    }
}
