//! Photos and graphics: decode, resize to display size, encode as AVIF.
//!
//! Thin facade over maintained crates: decoding and high-quality resizing use
//! the `image` crate, AVIF encoding uses `ravif` (the pure-Rust AV1 encoder
//! `rav1e` plus `avif-serialize`). Both dependencies are pure Rust and build
//! for native targets and for `wasm32-unknown-unknown`. The dependency survey
//! lives in `README.md`.

use std::fmt::{Display, Formatter};

use image::imageops::FilterType;

/// Lowest quality accepted by [`encode_avif`].
pub const QUALITY_MIN: f32 = 1.0;
/// Highest quality accepted by [`encode_avif`].
pub const QUALITY_MAX: f32 = 100.0;
/// Slowest encoder speed: best compression, longest encoding time.
pub const SPEED_MIN: u8 = 1;
/// Fastest encoder speed: worst compression, shortest encoding time.
pub const SPEED_MAX: u8 = 10;
/// Number of quality probes used by [`encode_avif_to_size`].
pub const TARGET_SIZE_STEPS: u32 = 8;

/// Encoded AVIF still image together with its pixel dimensions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvifImage {
    /// Complete AVIF file bytes (ISO-BMFF container holding an AV1 still).
    pub data: Vec<u8>,
    /// Output width in pixels after resizing.
    pub width: u32,
    /// Output height in pixels after resizing.
    pub height: u32,
}

impl AvifImage {
    /// Return the output dimensions as `(width, height)` in pixels.
    #[must_use]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Return the encoded file size in bytes.
    #[must_use]
    pub fn byte_size(&self) -> usize {
        self.data.len()
    }

    /// Return the output dimensions and byte size as one value.
    #[must_use]
    pub fn info(&self) -> OutputInfo {
        output_info(self)
    }
}

/// Output dimensions and byte size of an encoded image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputInfo {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Encoded file size in bytes.
    pub byte_size: usize,
}

/// Return the output dimensions and byte size of an encoded image.
#[must_use]
pub fn output_info(image: &AvifImage) -> OutputInfo {
    OutputInfo {
        width: image.width,
        height: image.height,
        byte_size: image.data.len(),
    }
}

/// Errors returned by this crate.
#[derive(Debug)]
pub enum Error {
    /// The input bytes are not a decodable PNG, JPEG or WebP image.
    Decode(image::ImageError),
    /// AVIF encoding failed.
    Encode(String),
    /// Requested quality is outside 1..=100.
    InvalidQuality(f32),
    /// Requested speed is outside 1..=10.
    InvalidSpeed(u8),
    /// Requested maximum dimension is zero.
    InvalidMaxDimension,
    /// Requested target byte size is zero.
    InvalidTargetSize,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Decode(error) => write!(f, "cannot decode input image: {error}"),
            Error::Encode(detail) => write!(f, "cannot encode AVIF image: {detail}"),
            Error::InvalidQuality(quality) => {
                write!(f, "quality {quality} is outside 1..=100")
            }
            Error::InvalidSpeed(speed) => write!(f, "speed {speed} is outside 1..=10"),
            Error::InvalidMaxDimension => write!(f, "maximum dimension must be positive"),
            Error::InvalidTargetSize => write!(f, "target byte size must be positive"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Decode(error) => Some(error),
            Error::Encode(_)
            | Error::InvalidQuality(_)
            | Error::InvalidSpeed(_)
            | Error::InvalidMaxDimension
            | Error::InvalidTargetSize => None,
        }
    }
}

impl From<image::ImageError> for Error {
    fn from(error: image::ImageError) -> Self {
        Error::Decode(error)
    }
}

impl From<ravif::Error> for Error {
    fn from(error: ravif::Error) -> Self {
        Error::Encode(error.to_string())
    }
}

/// Compute dimensions that fit inside `max_dim` while keeping the aspect ratio.
///
/// Images that already fit are returned unchanged; inputs are never upscaled.
/// Each side of a non-degenerate result is at least one pixel. Degenerate
/// input (a zero side or a zero `max_dim`) yields `(0, 0)`.
///
/// ```
/// assert_eq!(cprz_image::resized_dimensions(160, 90, 64), (64, 36));
/// assert_eq!(cprz_image::resized_dimensions(16, 12, 128), (16, 12));
/// ```
#[must_use]
pub fn resized_dimensions(src_width: u32, src_height: u32, max_dim: u32) -> (u32, u32) {
    if src_width == 0 || src_height == 0 || max_dim == 0 {
        return (0, 0);
    }
    let longest = src_width.max(src_height);
    if longest <= max_dim {
        return (src_width, src_height);
    }
    let scale = f64::from(max_dim) / f64::from(longest);
    let width = (f64::from(src_width) * scale).round() as u32;
    let height = (f64::from(src_height) * scale).round() as u32;
    (width.max(1), height.max(1))
}

/// Check the ISO-BMFF `ftyp` box for the AVIF brand (`avif` or `avis`).
#[must_use]
pub fn is_avif(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && bytes.get(4..8) == Some(b"ftyp".as_slice())
        && (bytes.get(8..12) == Some(b"avif".as_slice())
            || bytes.get(8..12) == Some(b"avis".as_slice()))
}

/// Decode PNG, JPEG or WebP bytes, resize to fit `max_dim`, encode AVIF.
///
/// `quality` is the `ravif` quality in 1..=100 (100 is best). `speed` is the
/// `rav1e` speed in 1..=10 (1 compresses best and runs slowest). Resizing uses
/// the high-quality Lanczos3 filter and keeps the aspect ratio; images that
/// already fit `max_dim` are not upscaled.
pub fn encode_avif(
    input: &[u8],
    max_dim: u32,
    quality: f32,
    speed: u8,
) -> Result<AvifImage, Error> {
    let max_dim = check_max_dim(max_dim)?;
    let quality = check_quality(quality)?;
    let speed = check_speed(speed)?;
    let (pixels, width, height) = decode_and_resize(input, max_dim)?;
    let data = encode_rgba(&pixels, width, height, quality, speed)?;
    Ok(AvifImage {
        data,
        width,
        height,
    })
}

/// Decode PNG, JPEG or WebP bytes, resize to fit `max_dim`, encode AVIF.
///
/// Uses the highest quality whose output fits in `target_bytes`, found by
/// binary search over the quality range 1..=100. Quality 100 is returned
/// directly when it already fits; when even quality 1 does not fit, the
/// quality-1 output is returned anyway. `speed` behaves as in [`encode_avif`].
pub fn encode_avif_to_size(
    input: &[u8],
    max_dim: u32,
    target_bytes: usize,
    speed: u8,
) -> Result<AvifImage, Error> {
    let max_dim = check_max_dim(max_dim)?;
    let speed = check_speed(speed)?;
    if target_bytes == 0 {
        return Err(Error::InvalidTargetSize);
    }
    let (pixels, width, height) = decode_and_resize(input, max_dim)?;
    let encode = |quality: f32| encode_rgba(&pixels, width, height, quality, speed);

    let best = encode(QUALITY_MAX)?;
    if best.len() <= target_bytes {
        return finish(best, width, height);
    }
    let mut current = encode(QUALITY_MIN)?;
    if current.len() > target_bytes {
        return finish(current, width, height);
    }
    let (mut low, mut high) = (QUALITY_MIN, QUALITY_MAX);
    for _ in 0..TARGET_SIZE_STEPS {
        let mid = 0.5 * (low + high);
        let candidate = encode(mid)?;
        if candidate.len() <= target_bytes {
            current = candidate;
            low = mid;
        } else {
            high = mid;
        }
    }
    finish(current, width, height)
}

fn finish(data: Vec<u8>, width: u32, height: u32) -> Result<AvifImage, Error> {
    Ok(AvifImage {
        data,
        width,
        height,
    })
}

fn check_max_dim(max_dim: u32) -> Result<u32, Error> {
    if max_dim > 0 {
        Ok(max_dim)
    } else {
        Err(Error::InvalidMaxDimension)
    }
}

fn check_quality(quality: f32) -> Result<f32, Error> {
    if quality.is_finite() && (QUALITY_MIN..=QUALITY_MAX).contains(&quality) {
        Ok(quality)
    } else {
        Err(Error::InvalidQuality(quality))
    }
}

fn check_speed(speed: u8) -> Result<u8, Error> {
    if (SPEED_MIN..=SPEED_MAX).contains(&speed) {
        Ok(speed)
    } else {
        Err(Error::InvalidSpeed(speed))
    }
}

/// Decode PNG, JPEG or WebP bytes and resize to fit `max_dim`, as RGBA pixels.
fn decode_and_resize(input: &[u8], max_dim: u32) -> Result<(Vec<ravif::RGBA8>, u32, u32), Error> {
    let decoded = image::load_from_memory(input)?;
    let (width, height) = resized_dimensions(decoded.width(), decoded.height(), max_dim);
    let pixels = if (width, height) == (decoded.width(), decoded.height()) {
        decoded.to_rgba8()
    } else {
        image::imageops::resize(&decoded, width, height, FilterType::Lanczos3)
    };
    let (width, height) = (pixels.width(), pixels.height());
    let (chunks, _) = pixels.as_chunks::<4>();
    let rgba = chunks
        .iter()
        .map(|chunk| ravif::RGBA8::new(chunk[0], chunk[1], chunk[2], chunk[3]))
        .collect();
    Ok((rgba, width, height))
}

fn encode_rgba(
    pixels: &[ravif::RGBA8],
    width: u32,
    height: u32,
    quality: f32,
    speed: u8,
) -> Result<Vec<u8>, Error> {
    let frame = ravif::Img::new(pixels, width as usize, height as usize);
    let encoded = ravif::Encoder::new()
        .with_quality(quality)
        .with_speed(speed)
        .encode_rgba(frame)?;
    Ok(encoded.avif_file)
}

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;
