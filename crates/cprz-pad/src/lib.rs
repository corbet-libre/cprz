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
#[path = "tests/lib.rs"]
mod tests;
