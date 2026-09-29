//! Facade: shrink any payload as far as possible, then pad it.
//!
//! Callers describe what they have ([`Payload`]) and what they need
//! ([`Goal`]); [`shrink`] picks the codec, converts where needed, compresses
//! at the highest effort, and optionally pads the result with `cprz-pad`.
//! [`expand`] reverses the process from the bytes alone: every packed payload
//! carries a self-describing container (magic, format version, kind, codec
//! id, padding flag, body length), so no side information is required.
//!
//! Order of operations is **convert → shrink → pad**. Encryption, when used,
//! happens after padding and outside this crate.
//!
//! # Example
//!
//! ```
//! let packed = cprz::shrink(
//!     cprz::Payload::Text("Hello, world!".repeat(100)),
//!     cprz::Goal::smallest(),
//! )?;
//! let back = cprz::expand(&packed)?;
//! assert_eq!(back, cprz::Payload::Text("Hello, world!".repeat(100)));
//! # Ok::<(), cprz::Error>(())
//! ```
//!
//! # Delta payloads
//!
//! A delta is relative to a base version by definition, so [`expand`] cannot
//! rebuild it alone and returns [`Error::DeltaNeedsBase`] for delta
//! containers. Use [`expand_delta`] with the same `old` version the delta was
//! built against. The container still identifies itself (kind and codec), so
//! the only extra input is the base data itself.
//!
//! # Extension points
//!
//! [`Kind::Audio`], [`Kind::Video`] and [`Kind::Structured`] reserve container
//! kind values for crates built in parallel (`cprz-audio`, `cprz-video`,
//! `cprz-structured`). This crate does not depend on them; packed payloads
//! that name those kinds decode to [`Error::UnimplementedKind`].

use std::fmt::{Display, Formatter};

/// Magic bytes starting every unpadded container: `b"CPRZ"`.
pub const MAGIC: [u8; 4] = *b"CPRZ";
/// Container format version written by [`shrink`].
pub const FORMAT_VERSION: u8 = 1;
/// Length of the container header in bytes: magic (4) + version (1) + kind
/// (1) + codec (1) + flags (1) + body length (4).
pub const HEADER_LEN: usize = 12;
/// Flag bit recording that the container bytes were padded with `cprz-pad`.
pub const FLAG_PADDED: u8 = 0x01;
/// Codec id for image bodies: AVIF still image (see `cprz-image`).
pub const IMAGE_CODEC_AVIF: u8 = 0x00;
/// Default AVIF quality for [`Goal::Smallest`] (`cprz-image` range 1..=100).
pub const DEFAULT_IMAGE_QUALITY: f32 = 70.0;
/// Default `rav1e` encoder speed for images (1..=10, 1 compresses best).
pub const DEFAULT_IMAGE_SPEED: u8 = 6;

/// Payload kind stored in the container header.
///
/// Numeric values are stable: never reuse a value for a different kind.
/// Audio, video and structured values are reserved for crates built in
/// parallel; this crate never emits them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// UTF-8 text, compressed by `cprz-text`.
    Text,
    /// Opaque bytes, compressed by `cprz-blob`.
    Bytes,
    /// Source image, resized and encoded as AVIF by `cprz-image`.
    Image,
    /// Increment of `new` against `old`, diffed by `cprz-delta`.
    Delta,
    /// Reserved for `cprz-audio`; not implemented here.
    Audio,
    /// Reserved for `cprz-video`; not implemented here.
    Video,
    /// Reserved for `cprz-structured`; not implemented here.
    Structured,
}

impl Kind {
    /// Return the header byte identifying this kind.
    #[must_use]
    pub fn as_byte(self) -> u8 {
        match self {
            Kind::Text => 0x00,
            Kind::Bytes => 0x01,
            Kind::Image => 0x02,
            Kind::Delta => 0x03,
            Kind::Audio => 0x10,
            Kind::Video => 0x11,
            Kind::Structured => 0x20,
        }
    }

    /// Parse a header byte into a kind.
    #[must_use]
    pub fn from_byte(byte: u8) -> Option<Kind> {
        match byte {
            0x00 => Some(Kind::Text),
            0x01 => Some(Kind::Bytes),
            0x02 => Some(Kind::Image),
            0x03 => Some(Kind::Delta),
            0x10 => Some(Kind::Audio),
            0x11 => Some(Kind::Video),
            0x20 => Some(Kind::Structured),
            _ => None,
        }
    }
}

impl Display for Kind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Text => write!(f, "text"),
            Kind::Bytes => write!(f, "bytes"),
            Kind::Image => write!(f, "image"),
            Kind::Delta => write!(f, "delta"),
            Kind::Audio => write!(f, "audio"),
            Kind::Video => write!(f, "video"),
            Kind::Structured => write!(f, "structured"),
        }
    }
}

/// Input image for [`Payload::Image`]: source bytes plus a display-size bound.
///
/// `data` holds PNG, JPEG or WebP bytes when shrinking. After [`expand`] it
/// holds the encoded AVIF file and `max_dim` echoes the bound used at encode
/// time. Image compression is lossy, so `expand(shrink(x)) != x` for images;
/// use [`cprz_image::is_avif`] to check the output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageInput {
    /// Source image bytes (shrink) or encoded AVIF bytes (after expand).
    pub data: Vec<u8>,
    /// Longest output side in pixels; images are never upscaled.
    pub max_dim: u32,
}

impl ImageInput {
    /// Build an image input from source bytes and a display-size bound.
    #[must_use]
    pub fn new(data: Vec<u8>, max_dim: u32) -> Self {
        Self { data, max_dim }
    }
}

/// Anything [`shrink`] accepts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Payload {
    /// UTF-8 text; smallest of zstd and Brotli wins (see `cprz-text`).
    Text(String),
    /// Opaque bytes; zstd with long-distance matching (see `cprz-blob`).
    Bytes(Vec<u8>),
    /// Source image; resized and encoded as AVIF (see `cprz-image`).
    Image(ImageInput),
    /// Increment of `new` against `old` (see `cprz-delta` and
    /// [`expand_delta`]).
    Delta {
        /// Base version the delta was built against.
        old: Vec<u8>,
        /// New version to reconstruct.
        new: Vec<u8>,
    },
}

impl Payload {
    /// Return the container [`Kind`] this payload shrinks into.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Payload::Text(_) => Kind::Text,
            Payload::Bytes(_) => Kind::Bytes,
            Payload::Image(_) => Kind::Image,
            Payload::Delta { .. } => Kind::Delta,
        }
    }
}

/// Named size limit carried as plain data, never as a network dependency.
///
/// Splitting and framing belong to the transport; profiles only state how
/// many bytes one unit carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetProfile {
    /// Profile name, e.g. `"veilid-message"`.
    pub name: &'static str,
    /// Maximum packed size in bytes for one unit of this profile.
    pub max_bytes: usize,
}

/// One 32 KiB Veilid message.
pub const VEILID_MESSAGE: TargetProfile = TargetProfile {
    name: "veilid-message",
    max_bytes: 32_768,
};

/// One 32 KiB Veilid DHT subkey value.
pub const VEILID_DHT_SUBKEY: TargetProfile = TargetProfile {
    name: "veilid-dht-subkey",
    max_bytes: 32_768,
};

/// All known target profiles.
pub const TARGET_PROFILES: [TargetProfile; 2] = [VEILID_MESSAGE, VEILID_DHT_SUBKEY];

/// Look up a target profile by name.
#[must_use]
pub fn target_profile(name: &str) -> Option<TargetProfile> {
    TARGET_PROFILES.iter().find(|p| p.name == name).copied()
}

/// What [`shrink`] should optimise for.
///
/// Padding with `cprz-pad` is applied last and is optional per goal: the
/// `_padded` constructors pad, the plain ones do not. Padding rounds the size
/// up, so a `Fit` or profile goal combined with padding fails with
/// [`Error::DoesNotFit`] when the padded size no longer fits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Goal {
    /// Smallest output, no size limit.
    Smallest {
        /// Pad the container with `cprz-pad` after shrinking.
        pad: bool,
    },
    /// Output must fit into `max_bytes`; images lower their AVIF quality to
    /// get there, other kinds fail with [`Error::DoesNotFit`].
    Fit {
        /// Maximum packed size in bytes.
        max_bytes: usize,
        /// Pad the container with `cprz-pad` after shrinking.
        pad: bool,
    },
    /// Output must fit into the named profile's limit (see
    /// [`TARGET_PROFILES`]).
    Profile {
        /// Size limit carried as plain data.
        profile: TargetProfile,
        /// Pad the container with `cprz-pad` after shrinking.
        pad: bool,
    },
}

impl Goal {
    /// Smallest output without padding.
    #[must_use]
    pub fn smallest() -> Self {
        Goal::Smallest { pad: false }
    }

    /// Smallest output, padded with `cprz-pad`.
    #[must_use]
    pub fn smallest_padded() -> Self {
        Goal::Smallest { pad: true }
    }

    /// Output must fit into `max_bytes`, without padding.
    #[must_use]
    pub fn fit(max_bytes: usize) -> Self {
        Goal::Fit {
            max_bytes,
            pad: false,
        }
    }

    /// Output must fit into `max_bytes`, padded with `cprz-pad`.
    #[must_use]
    pub fn fit_padded(max_bytes: usize) -> Self {
        Goal::Fit {
            max_bytes,
            pad: true,
        }
    }

    /// Output must fit into the profile's limit, without padding.
    #[must_use]
    pub fn for_profile(profile: TargetProfile) -> Self {
        Goal::Profile {
            profile,
            pad: false,
        }
    }

    /// Output must fit into the profile's limit, padded with `cprz-pad`.
    #[must_use]
    pub fn for_profile_padded(profile: TargetProfile) -> Self {
        Goal::Profile { profile, pad: true }
    }

    /// Size limit in bytes, if this goal carries one.
    #[must_use]
    pub fn max_bytes(self) -> Option<usize> {
        match self {
            Goal::Smallest { .. } => None,
            Goal::Fit { max_bytes, .. } => Some(max_bytes),
            Goal::Profile { profile, .. } => Some(profile.max_bytes),
        }
    }

    /// Whether this goal pads the container with `cprz-pad`.
    #[must_use]
    pub fn pad(self) -> bool {
        match self {
            Goal::Smallest { pad } | Goal::Fit { pad, .. } | Goal::Profile { pad, .. } => pad,
        }
    }
}

/// Packed output of [`shrink`]: a self-describing container, optionally padded.
///
/// The container is a 12-byte header (magic, format version, kind, codec id,
/// padding flag, body length) followed by the codec body. When the goal asked
/// for padding, the whole container is additionally framed with `cprz-pad`.
/// [`expand`] accepts both forms: it sniffs the magic bytes first (a padded
/// buffer starts with a zero length prefix, never with the magic) and unpads
/// otherwise.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Packed {
    bytes: Vec<u8>,
}

impl Packed {
    /// Wrap already-packed bytes after validating them.
    ///
    /// # Errors
    ///
    /// Returns the same errors [`expand`] returns for malformed input.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error> {
        parse(&bytes)?;
        Ok(Self { bytes })
    }

    /// Return the packed bytes (container, optionally padded).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume the wrapper and return the packed bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Return the packed length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Return true when there are no packed bytes (never true for valid
    /// output, which always holds at least the header).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Parse and return the container header.
    ///
    /// # Errors
    ///
    /// Returns the same errors [`expand`] returns for malformed input.
    pub fn header(&self) -> Result<Header, Error> {
        parse(&self.bytes).map(|(header, _)| header)
    }

    /// Return the payload [`Kind`] stored in the container.
    ///
    /// # Errors
    ///
    /// Returns the same errors [`expand`] returns for malformed input.
    pub fn kind(&self) -> Result<Kind, Error> {
        self.header().map(|header| header.kind)
    }

    /// Return true when the container was padded with `cprz-pad`.
    ///
    /// Unparseable input reports false; use [`Packed::header`] or
    /// [`Packed::from_bytes`] for the exact error.
    #[must_use]
    pub fn is_padded(&self) -> bool {
        self.header().is_ok_and(|header| header.padded)
    }
}

impl AsRef<[u8]> for Packed {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

/// Parsed container header (see [`Packed`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    /// Container format version (currently [`FORMAT_VERSION`]).
    pub version: u8,
    /// Payload kind.
    pub kind: Kind,
    /// Codec id: the sub-crate packet header byte (AVIF marker for images).
    pub codec: u8,
    /// True when the container bytes were padded with `cprz-pad`.
    pub padded: bool,
    /// Codec body length in bytes, excluding the header.
    pub body_len: u32,
}

/// Failures from shrinking, packing or expanding payloads.
#[derive(Debug)]
pub enum Error {
    /// The input holds no bytes at all.
    Empty,
    /// The magic bytes are wrong: not a `cprz` container and not a padded one.
    BadMagic,
    /// The container version is newer than this build understands.
    UnsupportedVersion(u8),
    /// The kind byte names no known [`Kind`].
    UnknownKind(u8),
    /// The kind is reserved for audio, video or structured payloads, whose
    /// crates are built in parallel and not wired here yet.
    UnimplementedKind(Kind),
    /// The packed kind is not a delta; only delta containers hold increments.
    NotADelta {
        /// Kind found in the container.
        found: Kind,
    },
    /// The codec byte names no codec for this kind.
    UnknownCodec {
        /// Kind found in the container.
        kind: Kind,
        /// Codec byte found in the container.
        codec: u8,
    },
    /// The container codec byte disagrees with the codec byte inside the body.
    CodecMismatch {
        /// Codec byte from the container header.
        header: u8,
        /// Codec byte from the body.
        body: u8,
    },
    /// Reserved flag bits are set; only [`FLAG_PADDED`] is defined.
    UnknownFlags(u8),
    /// The input ends before the header or body is complete.
    Truncated {
        /// Bytes required.
        expected: usize,
        /// Bytes available.
        found: usize,
    },
    /// Bytes trail the container body.
    TrailingBytes(usize),
    /// The codec body does not fit the 32-bit length field.
    TooLarge {
        /// Body length in bytes.
        bytes: usize,
    },
    /// The packed output does not fit the goal's size limit.
    DoesNotFit {
        /// Packed size in bytes.
        used: usize,
        /// Size limit in bytes.
        budget: usize,
    },
    /// A delta container needs its base version; use [`expand_delta`].
    DeltaNeedsBase,
    /// The image body is not a framed AVIF payload.
    InvalidImageBody,
    /// The `cprz-pad` framing around a padded container is corrupt.
    Unpad(cprz_pad::Error),
    /// The `cprz-text` layer failed.
    Text(cprz_text::Error),
    /// The `cprz-blob` layer failed.
    Blob(cprz_blob::Error),
    /// The `cprz-delta` layer failed.
    Delta(cprz_delta::Error),
    /// The `cprz-image` layer failed.
    Image(cprz_image::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "empty input"),
            Error::BadMagic => write!(f, "wrong magic bytes"),
            Error::UnsupportedVersion(version) => {
                write!(f, "unsupported container version {version}")
            }
            Error::UnknownKind(kind) => write!(f, "unknown payload kind {kind:#04X}"),
            Error::UnimplementedKind(kind) => {
                write!(f, "payload kind {kind} is not implemented in this build")
            }
            Error::NotADelta { found } => {
                write!(f, "not a delta container (found {found})")
            }
            Error::UnknownCodec { kind, codec } => {
                write!(f, "unknown codec {codec:#04X} for {kind} payloads")
            }
            Error::CodecMismatch { header, body } => write!(
                f,
                "container codec {header:#04X} disagrees with body codec {body:#04X}"
            ),
            Error::UnknownFlags(flags) => write!(f, "unknown container flags {flags:#04X}"),
            Error::Truncated { expected, found } => {
                write!(
                    f,
                    "truncated container: need {expected} bytes, found {found}"
                )
            }
            Error::TrailingBytes(extra) => {
                write!(f, "container has {extra} trailing bytes")
            }
            Error::TooLarge { bytes } => {
                write!(f, "codec body of {bytes} bytes exceeds the length field")
            }
            Error::DoesNotFit { used, budget } => {
                write!(f, "packed size {used} exceeds the budget of {budget} bytes")
            }
            Error::DeltaNeedsBase => {
                write!(f, "delta payload needs its base version (use expand_delta)")
            }
            Error::InvalidImageBody => write!(f, "image body is not a framed AVIF payload"),
            Error::Unpad(source) => write!(f, "padded framing is corrupt: {source}"),
            Error::Text(source) => write!(f, "text codec failed: {source}"),
            Error::Blob(source) => write!(f, "blob codec failed: {source}"),
            Error::Delta(source) => write!(f, "delta codec failed: {source}"),
            Error::Image(source) => write!(f, "image codec failed: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Unpad(source) => Some(source),
            Error::Text(source) => Some(source),
            Error::Blob(source) => Some(source),
            Error::Delta(source) => Some(source),
            Error::Image(source) => Some(source),
            _ => None,
        }
    }
}

impl From<cprz_pad::Error> for Error {
    fn from(source: cprz_pad::Error) -> Self {
        Error::Unpad(source)
    }
}

impl From<cprz_text::Error> for Error {
    fn from(source: cprz_text::Error) -> Self {
        Error::Text(source)
    }
}

impl From<cprz_blob::Error> for Error {
    fn from(source: cprz_blob::Error) -> Self {
        Error::Blob(source)
    }
}

impl From<cprz_delta::Error> for Error {
    fn from(source: cprz_delta::Error) -> Self {
        Error::Delta(source)
    }
}

impl From<cprz_image::Error> for Error {
    fn from(source: cprz_image::Error) -> Self {
        Error::Image(source)
    }
}

/// Parse packed bytes into their header and codec body.
///
/// Accepts raw containers (magic first) and padded ones (`cprz-pad` framing
/// first). Sniffing is unambiguous: a padded buffer starts with a big-endian
/// length prefix whose first bytes are zero for any allocatable size, never
/// with the magic.
fn parse(bytes: &[u8]) -> Result<(Header, Vec<u8>), Error> {
    if bytes.is_empty() {
        return Err(Error::Empty);
    }
    if bytes.len() >= HEADER_LEN && bytes[0..4] == MAGIC {
        parse_raw(bytes)
    } else {
        let inner = cprz_pad::unpad(bytes)?;
        parse_raw(&inner)
    }
}

/// Parse one raw container: header fields plus an exact-length body.
fn parse_raw(bytes: &[u8]) -> Result<(Header, Vec<u8>), Error> {
    if bytes.len() < HEADER_LEN {
        return Err(Error::Truncated {
            expected: HEADER_LEN,
            found: bytes.len(),
        });
    }
    if bytes[0..4] != MAGIC {
        return Err(Error::BadMagic);
    }
    let version = bytes[4];
    if version != FORMAT_VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let kind = Kind::from_byte(bytes[5]).ok_or(Error::UnknownKind(bytes[5]))?;
    let codec = bytes[6];
    let flags = bytes[7];
    if flags & !FLAG_PADDED != 0 {
        return Err(Error::UnknownFlags(flags));
    }
    let body_len = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let total = HEADER_LEN + body_len as usize;
    if bytes.len() < total {
        return Err(Error::Truncated {
            expected: total,
            found: bytes.len(),
        });
    }
    if bytes.len() > total {
        return Err(Error::TrailingBytes(bytes.len() - total));
    }
    Ok((
        Header {
            version,
            kind,
            codec,
            padded: flags & FLAG_PADDED != 0,
            body_len,
        },
        bytes[HEADER_LEN..total].to_vec(),
    ))
}

/// Check the body codec byte against the container codec byte.
fn check_codec(header_codec: u8, body: &[u8]) -> Result<(), Error> {
    let body_codec = body.first().ok_or(Error::Empty)?;
    if *body_codec != header_codec {
        return Err(Error::CodecMismatch {
            header: header_codec,
            body: *body_codec,
        });
    }
    Ok(())
}

/// Codec ids `cprz-text` may emit.
fn valid_text_codec(codec: u8) -> bool {
    matches!(
        codec,
        0x00..=0x02 // zstd, zstd with dictionary, Brotli (see cprz-text)
    )
}

/// Shrink a payload into a self-describing packed container.
///
/// Text, bytes and deltas compress losslessly; images resize to `max_dim` and
/// encode as AVIF (lossy). A `Fit` or profile goal caps the packed size:
/// images lower their quality towards the cap, other kinds fail with
/// [`Error::DoesNotFit`] when their smallest output does not fit.
///
/// # Errors
///
/// Returns [`Error::DoesNotFit`] when the packed output exceeds a `Fit` or
/// profile budget, [`Error::TooLarge`] when a codec body exceeds the length
/// field, or the sub-crate error when a codec fails.
pub fn shrink(payload: Payload, goal: Goal) -> Result<Packed, Error> {
    let budget = goal.max_bytes();
    let pad = goal.pad();
    let kind = payload.kind();
    let (codec, body) = encode_body(payload, budget)?;
    let body_len = u32::try_from(body.len()).map_err(|_| Error::TooLarge { bytes: body.len() })?;
    let mut container = Vec::with_capacity(HEADER_LEN + body.len());
    container.extend_from_slice(&MAGIC);
    container.push(FORMAT_VERSION);
    container.push(kind.as_byte());
    container.push(codec);
    container.push(if pad { FLAG_PADDED } else { 0 });
    container.extend_from_slice(&body_len.to_be_bytes());
    container.extend_from_slice(&body);
    let bytes = if pad {
        cprz_pad::pad(&container)
    } else {
        container
    };
    if let Some(limit) = budget.filter(|&limit| bytes.len() > limit) {
        return Err(Error::DoesNotFit {
            used: bytes.len(),
            budget: limit,
        });
    }
    Ok(Packed { bytes })
}

/// Encode one payload into its codec body, honouring the size budget.
fn encode_body(payload: Payload, budget: Option<usize>) -> Result<(u8, Vec<u8>), Error> {
    match payload {
        Payload::Text(text) => {
            let body = cprz_text::compress(&text, None)?;
            let codec = body.first().ok_or(Error::Empty)?;
            Ok((*codec, body))
        }
        Payload::Bytes(bytes) => {
            let body = cprz_blob::compress(&bytes)?;
            let codec = body.first().ok_or(Error::Empty)?;
            Ok((*codec, body))
        }
        Payload::Delta { old, new } => {
            let body = cprz_delta::diff(&old, &new);
            let codec = body.first().ok_or(Error::Empty)?;
            Ok((*codec, body))
        }
        Payload::Image(input) => encode_image(&input, budget),
    }
}

/// Resize and encode an image, targeting the codec budget when one is set.
///
/// The budget covers the full packed size; the codec target subtracts the
/// container header. [`shrink`] checks the full packed size (and the padded
/// size) afterwards, so an image whose lowest quality still overshoots fails
/// with [`Error::DoesNotFit`].
fn encode_image(input: &ImageInput, budget: Option<usize>) -> Result<(u8, Vec<u8>), Error> {
    if input.max_dim == 0 {
        return Err(Error::Image(cprz_image::Error::InvalidMaxDimension));
    }
    let encoded = match budget {
        Some(limit) => {
            let target = limit.saturating_sub(HEADER_LEN);
            if target == 0 {
                return Err(Error::DoesNotFit {
                    used: HEADER_LEN + 1,
                    budget: limit,
                });
            }
            cprz_image::encode_avif_to_size(
                &input.data,
                input.max_dim,
                target,
                DEFAULT_IMAGE_SPEED,
            )?
        }
        None => cprz_image::encode_avif(
            &input.data,
            input.max_dim,
            DEFAULT_IMAGE_QUALITY,
            DEFAULT_IMAGE_SPEED,
        )?,
    };
    // Image bodies frame the encode bound so expand can echo it: the 4-byte
    // big-endian max_dim, the AVIF codec byte, then the AVIF file.
    let mut body = Vec::with_capacity(4 + 1 + encoded.data.len());
    body.extend_from_slice(&input.max_dim.to_be_bytes());
    body.push(IMAGE_CODEC_AVIF);
    body.extend_from_slice(&encoded.data);
    Ok((IMAGE_CODEC_AVIF, body))
}

/// Expand a packed payload back to its original form.
///
/// Needs no side information: the container names its kind and codec. Image
/// output holds the encoded AVIF file (compression is lossy). Delta
/// containers fail with [`Error::DeltaNeedsBase`]; use [`expand_delta`] with
/// the base version instead.
///
/// # Errors
///
/// Returns an error for malformed containers (wrong magic, truncation,
/// trailing bytes, unknown kind/codec/version/flags, codec failures) and
/// [`Error::UnimplementedKind`] for the reserved audio, video and structured
/// kinds.
pub fn expand(packed: &Packed) -> Result<Payload, Error> {
    let (header, body) = parse(&packed.bytes)?;
    match header.kind {
        Kind::Text => {
            if !valid_text_codec(header.codec) {
                return Err(Error::UnknownCodec {
                    kind: header.kind,
                    codec: header.codec,
                });
            }
            check_codec(header.codec, &body)?;
            let text = cprz_text::decompress(&body, None)?;
            Ok(Payload::Text(text))
        }
        Kind::Bytes => {
            if header.codec != 0x00 {
                return Err(Error::UnknownCodec {
                    kind: header.kind,
                    codec: header.codec,
                });
            }
            check_codec(header.codec, &body)?;
            let bytes = cprz_blob::decompress(&body)?;
            Ok(Payload::Bytes(bytes))
        }
        Kind::Image => expand_image(&header, &body),
        Kind::Delta => Err(Error::DeltaNeedsBase),
        Kind::Audio | Kind::Video | Kind::Structured => Err(Error::UnimplementedKind(header.kind)),
    }
}

/// Expand an image container into its AVIF file plus the encode bound.
fn expand_image(header: &Header, body: &[u8]) -> Result<Payload, Error> {
    if header.codec != IMAGE_CODEC_AVIF {
        return Err(Error::UnknownCodec {
            kind: header.kind,
            codec: header.codec,
        });
    }
    if body.len() < 5 {
        return Err(Error::Truncated {
            expected: 5,
            found: body.len(),
        });
    }
    let max_dim = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    if body[4] != IMAGE_CODEC_AVIF {
        return Err(Error::CodecMismatch {
            header: header.codec,
            body: body[4],
        });
    }
    let avif = &body[5..];
    if !cprz_image::is_avif(avif) {
        return Err(Error::InvalidImageBody);
    }
    Ok(Payload::Image(ImageInput {
        data: avif.to_vec(),
        max_dim,
    }))
}

/// Rebuild the new version from a delta container and its base version.
///
/// Accepts both delta-mode and full-mode containers (see `cprz-delta`): full
/// mode ignores `old`, delta mode fails when `old` is not the version the
/// delta was built against.
///
/// # Errors
///
/// Returns [`Error::NotADelta`] when the container holds another kind, and
/// the usual container or codec errors for malformed input.
pub fn expand_delta(old: &[u8], packed: &Packed) -> Result<Vec<u8>, Error> {
    let (header, body) = parse(&packed.bytes)?;
    if header.kind != Kind::Delta {
        return Err(Error::NotADelta { found: header.kind });
    }
    if header.codec != cprz_delta::MODE_DELTA && header.codec != cprz_delta::MODE_FULL {
        return Err(Error::UnknownCodec {
            kind: header.kind,
            codec: header.codec,
        });
    }
    check_codec(header.codec, &body)?;
    Ok(cprz_delta::apply(old, &body)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;

    /// Technical paragraph repeated like log output.
    const PARAGRAPH: &str = "Packet switching splits each message into small blocks that travel \
        independently across the network and are reassembled at the destination. Sequence numbers \
        let the receiver restore the original order even when blocks arrive out of order. ";

    fn long_text() -> String {
        PARAGRAPH.repeat(64)
    }

    /// Deterministic pseudo-random bytes that barely compress.
    fn random_bytes(len: usize) -> Vec<u8> {
        let mut state: u32 = 0x1234_5678;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state & 0xff) as u8
            })
            .collect()
    }

    /// Checkerboard blocks: highly compressible for both PNG and AVIF.
    fn blocks_png(width: u32, height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
            let on = (x / 16 + y / 16) % 2 == 0;
            let v = if on { 30 } else { 220 };
            Rgb([v, v, v])
        }));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("test image encodes");
        bytes
    }

    #[test]
    fn text_round_trip_with_size_assertion() {
        let text = long_text();
        let packed = shrink(Payload::Text(text.clone()), Goal::smallest()).unwrap();
        assert_eq!(packed.kind().unwrap(), Kind::Text);
        assert_eq!(expand(&packed).unwrap(), Payload::Text(text.clone()));
        assert!(
            packed.len() < text.len() / 2,
            "packed {} vs input {}",
            packed.len(),
            text.len()
        );
    }

    #[test]
    fn empty_text_round_trip() {
        let packed = shrink(Payload::Text(String::new()), Goal::smallest()).unwrap();
        assert_eq!(expand(&packed).unwrap(), Payload::Text(String::new()));
    }

    #[test]
    fn bytes_round_trip_with_size_assertion() {
        let input: Vec<u8> = (0..20_000).map(|i| (i % 251) as u8).collect();
        let packed = shrink(Payload::Bytes(input.clone()), Goal::smallest()).unwrap();
        assert_eq!(packed.kind().unwrap(), Kind::Bytes);
        assert_eq!(expand(&packed).unwrap(), Payload::Bytes(input.clone()));
        assert!(
            packed.len() < input.len() / 2,
            "packed {} vs input {}",
            packed.len(),
            input.len()
        );
    }

    #[test]
    fn image_round_trip_reports_avif() {
        let input = blocks_png(128, 128);
        let packed = shrink(
            Payload::Image(ImageInput::new(input.clone(), 64)),
            Goal::smallest(),
        )
        .unwrap();
        assert_eq!(packed.kind().unwrap(), Kind::Image);
        match expand(&packed).unwrap() {
            Payload::Image(out) => {
                assert!(cprz_image::is_avif(&out.data));
                assert_eq!(out.max_dim, 64);
                assert!(
                    packed.len() < input.len(),
                    "packed {} vs input {}",
                    packed.len(),
                    input.len()
                );
            }
            other => panic!("expected image, got {}", other.kind()),
        }
    }

    #[test]
    fn image_fit_into_n_is_honoured() {
        let input = blocks_png(96, 96);
        let budget = 3000;
        let packed = shrink(
            Payload::Image(ImageInput::new(input, 64)),
            Goal::fit(budget),
        )
        .unwrap();
        assert!(
            packed.len() <= budget,
            "packed {} exceeds budget {budget}",
            packed.len()
        );
        match expand(&packed).unwrap() {
            Payload::Image(out) => assert!(cprz_image::is_avif(&out.data)),
            other => panic!("expected image, got {}", other.kind()),
        }
    }

    #[test]
    fn delta_round_trip_through_expand_delta() {
        let old = long_text().into_bytes();
        let mut new = old.clone();
        new.splice(1000..1011, b"swift red wolf".iter().copied());
        new.extend_from_slice(b"One more line at the end.");
        let packed = shrink(
            Payload::Delta {
                old: old.clone(),
                new: new.clone(),
            },
            Goal::smallest(),
        )
        .unwrap();
        assert_eq!(packed.kind().unwrap(), Kind::Delta);
        // Deltas need their base version: plain expand says so explicitly.
        assert!(matches!(expand(&packed), Err(Error::DeltaNeedsBase)));
        assert_eq!(expand_delta(&old, &packed).unwrap(), new);
        assert!(
            packed.len() * 20 < new.len(),
            "packed {} is not tiny against {}",
            packed.len(),
            new.len()
        );
    }

    #[test]
    fn fit_rejects_incompressible_input() {
        let input = random_bytes(1000);
        let err = shrink(Payload::Bytes(input), Goal::fit(100)).unwrap_err();
        assert!(matches!(err, Error::DoesNotFit { .. }), "got {err}");
    }

    #[test]
    fn profiles_are_plain_data_with_limits() {
        assert_eq!(VEILID_MESSAGE.max_bytes, 32_768);
        assert_eq!(VEILID_DHT_SUBKEY.max_bytes, 32_768);
        assert_eq!(target_profile("veilid-message"), Some(VEILID_MESSAGE));
        assert_eq!(target_profile("veilid-dht-subkey"), Some(VEILID_DHT_SUBKEY));
        assert_eq!(target_profile("no-such-profile"), None);

        let text = long_text();
        let packed = shrink(
            Payload::Text(text.clone()),
            Goal::for_profile(VEILID_MESSAGE),
        )
        .unwrap();
        assert!(packed.len() <= VEILID_MESSAGE.max_bytes);
        assert_eq!(expand(&packed).unwrap(), Payload::Text(text));
    }

    #[test]
    fn padding_round_trip_marks_the_flag() {
        let text = long_text();
        let packed = shrink(Payload::Text(text.clone()), Goal::smallest_padded()).unwrap();
        assert!(packed.is_padded());
        assert!(packed.header().unwrap().padded);
        assert_eq!(expand(&packed).unwrap(), Payload::Text(text));

        let raw = shrink(Payload::Text("pad me".repeat(20)), Goal::smallest()).unwrap();
        assert!(!raw.is_padded());
    }

    #[test]
    fn container_rejects_corrupted_or_unknown_input() {
        let text = long_text();
        let packed = shrink(Payload::Text(text), Goal::smallest()).unwrap();
        let bytes = packed.into_bytes();

        // Wrong magic.
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert!(matches!(
            Packed::from_bytes(bad),
            Err(Error::Unpad(_) | Error::BadMagic)
        ));

        // Truncated header and truncated body.
        assert!(matches!(
            Packed::from_bytes(bytes[..5].to_vec()),
            Err(Error::Unpad(_) | Error::Truncated { .. })
        ));
        assert!(matches!(
            Packed::from_bytes(bytes[..bytes.len() - 1].to_vec()),
            Err(Error::Truncated { .. })
        ));

        // Unknown version, kind, flags and codec.
        let mut bad = bytes.clone();
        bad[4] = 0x7F;
        assert!(matches!(
            Packed::from_bytes(bad),
            Err(Error::UnsupportedVersion(0x7F))
        ));
        let mut bad = bytes.clone();
        bad[5] = 0x09;
        assert!(matches!(
            Packed::from_bytes(bad),
            Err(Error::UnknownKind(0x09))
        ));
        let mut bad = bytes.clone();
        bad[7] = 0x02;
        assert!(matches!(
            Packed::from_bytes(bad),
            Err(Error::UnknownFlags(0x02))
        ));
        let mut bad = bytes.clone();
        bad[6] = 0x09;
        let bad = Packed { bytes: bad };
        assert!(matches!(
            expand(&bad),
            Err(Error::UnknownCodec { .. } | Error::CodecMismatch { .. })
        ));

        // Trailing garbage.
        let mut bad = bytes.clone();
        bad.push(0xAA);
        assert!(matches!(
            Packed::from_bytes(bad),
            Err(Error::TrailingBytes(1))
        ));

        // Corrupted body codec byte disagrees with the container header.
        let mut tampered = bytes.clone();
        tampered[HEADER_LEN] ^= 0xFF;
        assert!(matches!(
            expand(&Packed { bytes: tampered }),
            Err(Error::CodecMismatch { .. } | Error::UnknownCodec { .. })
        ));

        // Empty and garbage input.
        assert!(matches!(Packed::from_bytes(vec![]), Err(Error::Empty)));
        assert!(Packed::from_bytes(b"hello, world".to_vec()).is_err());
    }

    #[test]
    fn reserved_kinds_report_unimplemented() {
        // A hand-framed audio container parses but has no codec wired here.
        let mut container = Vec::new();
        container.extend_from_slice(&MAGIC);
        container.push(FORMAT_VERSION);
        container.push(Kind::Audio.as_byte());
        container.push(0x00);
        container.push(0x00);
        container.extend_from_slice(&1u32.to_be_bytes());
        container.push(0x00);
        let packed = Packed::from_bytes(container).unwrap();
        assert_eq!(packed.kind().unwrap(), Kind::Audio);
        assert!(matches!(
            expand(&packed),
            Err(Error::UnimplementedKind(Kind::Audio))
        ));
    }

    #[test]
    fn expand_delta_rejects_non_delta_containers() {
        let packed = shrink(Payload::Bytes(vec![1, 2, 3]), Goal::smallest()).unwrap();
        assert!(matches!(
            expand_delta(&[], &packed),
            Err(Error::NotADelta { .. })
        ));
    }

    #[test]
    fn goal_accessors_report_limits_and_padding() {
        assert_eq!(Goal::smallest().max_bytes(), None);
        assert!(!Goal::smallest().pad());
        assert!(Goal::smallest_padded().pad());
        assert_eq!(Goal::fit(100).max_bytes(), Some(100));
        assert!(Goal::fit_padded(100).pad());
        assert_eq!(Goal::for_profile(VEILID_MESSAGE).max_bytes(), Some(32_768));
        assert!(Goal::for_profile_padded(VEILID_MESSAGE).pad());
    }

    #[test]
    fn kind_bytes_are_stable() {
        assert_eq!(Kind::Text.as_byte(), 0x00);
        assert_eq!(Kind::Bytes.as_byte(), 0x01);
        assert_eq!(Kind::Image.as_byte(), 0x02);
        assert_eq!(Kind::Delta.as_byte(), 0x03);
        assert_eq!(Kind::Audio.as_byte(), 0x10);
        assert_eq!(Kind::Video.as_byte(), 0x11);
        assert_eq!(Kind::Structured.as_byte(), 0x20);
        assert_eq!(Kind::from_byte(0x09), None);
    }
}
