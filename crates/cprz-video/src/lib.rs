//! Short video clips: encode raw RGB or YUV frames as AV1 in WebM.
//!
//! Thin facade over the maintained pure-Rust AV1 encoder `rav1e`. The
//! minimal WebM (EBML) muxer, the RGB-to-YUV converter and the
//! container-level probe are the only logic owned by this crate: no
//! maintained pure-Rust WebM muxer with AV1 support was found. The
//! dependency survey lives in the crate `README.md`.
//!
//! There is no pixel-level decoder yet. No AV1 decoder exists that is at
//! once maintained, license-compatible with this workspace and buildable
//! for `wasm32-unknown-unknown` without extra toolchains, so decoding is
//! left to browsers (WebCodecs, `<video>`). [`probe`] reads the dimensions
//! and the frame count back from the container without decoding pixels.

use std::fmt::{Display, Formatter};

use rav1e::prelude::*;

/// Lowest quantizer accepted: best quality, largest output.
pub const QUANTIZER_MIN: usize = 0;
/// Highest quantizer accepted: worst quality, smallest output.
pub const QUANTIZER_MAX: usize = 255;
/// Default quantizer used by [`EncodeOptions::default`].
pub const QUANTIZER_DEFAULT: usize = 100;
/// Slowest encoder speed: best compression, longest encoding time.
pub const SPEED_MIN: u8 = 0;
/// Fastest encoder speed: worst compression, shortest encoding time.
pub const SPEED_MAX: u8 = 10;
/// Default encoder speed: high effort while staying fast on tiny clips.
pub const DEFAULT_SPEED: u8 = 4;
/// Smallest bitrate accepted, in kbit/s.
pub const BITRATE_MIN_KBPS: u32 = 1;
/// Largest frame side accepted, in pixels (AV1 level 6 limit class).
pub const MAX_DIMENSION: u32 = 16384;

/// Errors returned by this crate.
#[derive(Debug)]
pub enum Error {
    /// The clip contains no frames.
    EmptyClip,
    /// Width or height is zero or larger than [`MAX_DIMENSION`].
    InvalidDimensions {
        /// Requested frame width in pixels.
        width: u32,
        /// Requested frame height in pixels.
        height: u32,
    },
    /// Frame rate numerator or denominator is zero.
    InvalidFps {
        /// Requested frames per second, numerator.
        num: u32,
        /// Requested frames per second, denominator.
        den: u32,
    },
    /// Quantizer is larger than [`QUANTIZER_MAX`].
    InvalidQuantizer(usize),
    /// Speed is larger than [`SPEED_MAX`].
    InvalidSpeed(u8),
    /// Bitrate is smaller than [`BITRATE_MIN_KBPS`] or too large to express.
    InvalidBitrate(u32),
    /// A frame buffer does not match the clip dimensions.
    FrameSizeMismatch {
        /// Index of the offending frame.
        index: usize,
        /// Expected buffer length in bytes.
        expected: usize,
        /// Actual buffer length in bytes.
        actual: usize,
    },
    /// The encoder rejected the configuration or failed while encoding.
    Encoder(String),
    /// The bytes are not a WebM container this crate understands.
    InvalidContainer(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::EmptyClip => write!(f, "clip contains no frames"),
            Error::InvalidDimensions { width, height } => write!(
                f,
                "invalid dimensions {width}x{height}: sides must be 1..={MAX_DIMENSION}"
            ),
            Error::InvalidFps { num, den } => {
                write!(
                    f,
                    "invalid frame rate {num}/{den}: both parts must be positive"
                )
            }
            Error::InvalidQuantizer(quantizer) => write!(
                f,
                "quantizer {quantizer} is outside {}..={QUANTIZER_MAX}",
                QUANTIZER_MIN
            ),
            Error::InvalidSpeed(speed) => {
                write!(f, "speed {speed} is outside {SPEED_MIN}..={SPEED_MAX}")
            }
            Error::InvalidBitrate(bitrate) => {
                write!(f, "bitrate {bitrate} kbit/s is below {BITRATE_MIN_KBPS}")
            }
            Error::FrameSizeMismatch {
                index,
                expected,
                actual,
            } => write!(
                f,
                "frame {index} has {actual} bytes, expected {expected} for these dimensions"
            ),
            Error::Encoder(detail) => write!(f, "AV1 encoding failed: {detail}"),
            Error::InvalidContainer(detail) => {
                write!(f, "not a WebM container from this crate: {detail}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// One raw input frame, either packed RGB or planar YUV 4:2:0.
///
/// `Rgb8` holds `width * height * 3` bytes in row-major RGB order.
/// `Yuv420` holds 8-bit BT.601 planes: `y` has `width * height` bytes and
/// `u`/`v` have `((width + 1) / 2) * ((height + 1) / 2)` bytes each.
#[derive(Clone, Copy, Debug)]
pub enum RawFrame<'a> {
    /// Packed 8-bit RGB bytes.
    Rgb8(&'a [u8]),
    /// Planar 8-bit 4:2:0 YUV bytes.
    Yuv420 {
        /// Luma plane, `width * height` bytes.
        y: &'a [u8],
        /// Chroma plane, quarter resolution (rounded up).
        u: &'a [u8],
        /// Chroma plane, quarter resolution (rounded up).
        v: &'a [u8],
    },
}

/// A clip to encode: dimensions, frame rate and raw frames.
///
/// The frame rate is `fps_num / fps_den` frames per second, for example
/// 30/1 for 30 fps or 30000/1001 for NTSC.
#[derive(Clone, Debug)]
pub struct Clip<'a> {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frame rate numerator.
    pub fps_num: u32,
    /// Frame rate denominator.
    pub fps_den: u32,
    /// Raw frames in presentation order.
    pub frames: Vec<RawFrame<'a>>,
}

impl<'a> Clip<'a> {
    /// Build a clip from dimensions, frame rate and raw frames.
    #[must_use]
    pub fn new(
        width: u32,
        height: u32,
        fps_num: u32,
        fps_den: u32,
        frames: Vec<RawFrame<'a>>,
    ) -> Self {
        Clip {
            width,
            height,
            fps_num,
            fps_den,
            frames,
        }
    }

    /// Return the number of frames in the clip.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// Rate control: constant quality or capped bitrate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateControl {
    /// Constant quality; 0 is best, 255 is smallest.
    Quality {
        /// Base quantizer, 0..=255.
        quantizer: usize,
    },
    /// Single-pass capped bitrate mode.
    Bitrate {
        /// Target bitrate in kbit/s.
        bitrate_kbps: u32,
    },
}

/// Encoder settings: rate control plus the speed/quality trade-off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodeOptions {
    /// Target quality or bitrate.
    pub rate: RateControl,
    /// `rav1e` speed preset 0..=10; 0 compresses best and runs slowest.
    pub speed: u8,
}

impl EncodeOptions {
    /// Constant-quality options with an explicit speed preset.
    #[must_use]
    pub fn quality(quantizer: usize, speed: u8) -> Self {
        EncodeOptions {
            rate: RateControl::Quality { quantizer },
            speed,
        }
    }

    /// Capped-bitrate options with an explicit speed preset.
    #[must_use]
    pub fn bitrate(bitrate_kbps: u32, speed: u8) -> Self {
        EncodeOptions {
            rate: RateControl::Bitrate { bitrate_kbps },
            speed,
        }
    }

    /// Default constant-quality options at high-effort speed.
    #[must_use]
    pub fn default_quality() -> Self {
        EncodeOptions::quality(QUANTIZER_DEFAULT, DEFAULT_SPEED)
    }
}

impl Default for EncodeOptions {
    /// Default constant-quality options at high-effort speed.
    fn default() -> Self {
        EncodeOptions::default_quality()
    }
}

/// An encoded clip: complete WebM bytes plus its shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedVideo {
    /// Complete WebM file bytes (AV1 in a browser-playable container).
    pub data: Vec<u8>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Number of encoded frames.
    pub frames: usize,
}

impl EncodedVideo {
    /// Return the frame dimensions as `(width, height)` in pixels.
    #[must_use]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Return the encoded file size in bytes.
    #[must_use]
    pub fn byte_size(&self) -> usize {
        self.data.len()
    }

    /// Return the number of encoded frames.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames
    }

    /// Return dimensions and frame count as one value.
    #[must_use]
    pub fn info(&self) -> VideoInfo {
        VideoInfo {
            width: self.width,
            height: self.height,
            frames: self.frames,
        }
    }
}

/// Dimensions and frame count read back from an encoded container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoInfo {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Number of frames in the container.
    pub frames: usize,
}

/// Encode raw RGB frames directly.
///
/// `rgb_frames` holds one packed `width * height * 3` RGB buffer per frame.
/// See [`encode`] for the general entry point accepting YUV input as well.
pub fn encode_rgb(
    rgb_frames: &[Vec<u8>],
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
    options: &EncodeOptions,
) -> Result<EncodedVideo, Error> {
    let frames = rgb_frames
        .iter()
        .map(Vec::as_slice)
        .map(RawFrame::Rgb8)
        .collect();
    encode(&Clip::new(width, height, fps_num, fps_den, frames), options)
}

/// Encode a clip as AV1 in a WebM container.
///
/// Quality mode (`RateControl::Quality`) picks a constant quantizer; lower
/// is better and larger. Bitrate mode (`RateControl::Bitrate`) aims at the
/// target bitrate in kbit/s. Lower `speed` values compress better and run
/// slower; [`DEFAULT_SPEED`] is the high-effort default.
pub fn encode(clip: &Clip<'_>, options: &EncodeOptions) -> Result<EncodedVideo, Error> {
    let width = clip.width;
    let height = clip.height;
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::InvalidDimensions { width, height });
    }
    if clip.fps_num == 0 || clip.fps_den == 0 {
        return Err(Error::InvalidFps {
            num: clip.fps_num,
            den: clip.fps_den,
        });
    }
    if clip.frames.is_empty() {
        return Err(Error::EmptyClip);
    }
    let speed = check_speed(options.speed)?;
    let (quantizer, bitrate) = match options.rate {
        RateControl::Quality { quantizer } => {
            if quantizer > QUANTIZER_MAX {
                return Err(Error::InvalidQuantizer(quantizer));
            }
            (quantizer, 0)
        }
        RateControl::Bitrate { bitrate_kbps } => {
            if bitrate_kbps < BITRATE_MIN_KBPS {
                return Err(Error::InvalidBitrate(bitrate_kbps));
            }
            // rav1e takes the target in bits per second.
            let bitrate = i32::try_from(u64::from(bitrate_kbps) * 1000)
                .map_err(|_| Error::InvalidBitrate(bitrate_kbps))?;
            (QUANTIZER_DEFAULT, bitrate)
        }
    };

    let w = width as usize;
    let h = height as usize;
    let config = EncoderConfig {
        width: w,
        height: h,
        time_base: Rational::new(u64::from(clip.fps_den), u64::from(clip.fps_num)),
        bit_depth: 8,
        chroma_sampling: ChromaSampling::Cs420,
        quantizer,
        bitrate,
        speed_settings: SpeedSettings::from_preset(speed),
        ..Default::default()
    };
    let context = Config::default()
        .with_encoder_config(config)
        .with_threads(1);
    let mut encoder: Context<u8> = context
        .new_context()
        .map_err(|error| Error::Encoder(format!("invalid encoder config: {error:?}")))?;
    // Sequence header in Matroska/ISOBMFF layout, stored as CodecPrivate so
    // browsers can initialise the AV1 decoder before the first frame.
    let codec_private = encoder.container_sequence_header();

    let uv_w = w.div_ceil(2);
    let uv_h = h.div_ceil(2);
    for (index, frame) in clip.frames.iter().enumerate() {
        let (y, u, v) = sample_frame(frame, index, w, h, uv_w, uv_h)?;
        let mut raw = encoder.new_frame();
        raw.planes[0].copy_from_raw_u8(&y, w, 1);
        raw.planes[1].copy_from_raw_u8(&u, uv_w, 1);
        raw.planes[2].copy_from_raw_u8(&v, uv_w, 1);
        encoder
            .send_frame(raw)
            .map_err(|error| Error::Encoder(format!("cannot send frame {index}: {error:?}")))?;
    }
    encoder.flush();

    // Collect one packet per shown frame: (is_keyframe, AV1 OBU bytes).
    let mut packets: Vec<(bool, Vec<u8>)> = Vec::with_capacity(clip.frames.len());
    loop {
        match encoder.receive_packet() {
            Ok(packet) => packets.push((packet.frame_type == FrameType::KEY, packet.data)),
            Err(EncoderStatus::LimitReached) => break,
            Err(EncoderStatus::Encoded) | Err(EncoderStatus::NeedMoreData) => {}
            Err(error) => {
                return Err(Error::Encoder(format!("encoding failed: {error:?}")));
            }
        }
    }
    if packets.is_empty() {
        return Err(Error::Encoder("encoder produced no packets".to_owned()));
    }

    let data = mux_webm(
        width,
        height,
        clip.fps_num,
        clip.fps_den,
        &codec_private,
        &packets,
    );
    Ok(EncodedVideo {
        data,
        width,
        height,
        frames: packets.len(),
    })
}

fn check_speed(speed: u8) -> Result<u8, Error> {
    if (SPEED_MIN..=SPEED_MAX).contains(&speed) {
        Ok(speed)
    } else {
        Err(Error::InvalidSpeed(speed))
    }
}

/// Eight-bit 4:2:0 planes: full-resolution luma plus quarter-resolution chroma.
type Yuv420 = (Vec<u8>, Vec<u8>, Vec<u8>);

/// Convert one input frame to its 8-bit 4:2:0 planes, checking sizes.
fn sample_frame(
    frame: &RawFrame<'_>,
    index: usize,
    w: usize,
    h: usize,
    uv_w: usize,
    uv_h: usize,
) -> Result<Yuv420, Error> {
    match frame {
        RawFrame::Rgb8(rgb) => {
            let expected = w * h * 3;
            if rgb.len() != expected {
                return Err(Error::FrameSizeMismatch {
                    index,
                    expected,
                    actual: rgb.len(),
                });
            }
            Ok(rgb_to_yuv420(rgb, w, h))
        }
        RawFrame::Yuv420 { y, u, v } => {
            if y.len() != w * h {
                return Err(Error::FrameSizeMismatch {
                    index,
                    expected: w * h,
                    actual: y.len(),
                });
            }
            if u.len() != uv_w * uv_h || v.len() != uv_w * uv_h {
                return Err(Error::FrameSizeMismatch {
                    index,
                    expected: uv_w * uv_h,
                    actual: if u.len() != uv_w * uv_h {
                        u.len()
                    } else {
                        v.len()
                    },
                });
            }
            Ok((y.to_vec(), u.to_vec(), v.to_vec()))
        }
    }
}

/// Convert packed RGB to full-range BT.601 YUV 4:2:0 with 2x2 box filtering.
fn rgb_to_yuv420(rgb: &[u8], w: usize, h: usize) -> Yuv420 {
    let uv_w = w.div_ceil(2);
    let uv_h = h.div_ceil(2);
    let mut y = vec![0u8; w * h];
    let mut u_sum = vec![0f32; uv_w * uv_h];
    let mut v_sum = vec![0f32; uv_w * uv_h];
    let mut count = vec![0u32; uv_w * uv_h];
    for row in 0..h {
        for col in 0..w {
            let i = row * w + col;
            let r = f32::from(rgb[3 * i]);
            let g = f32::from(rgb[3 * i + 1]);
            let b = f32::from(rgb[3 * i + 2]);
            y[i] = (0.299 * r + 0.587 * g + 0.114 * b)
                .round()
                .clamp(0.0, 255.0) as u8;
            let block = (row / 2) * uv_w + col / 2;
            u_sum[block] += -0.168_736 * r - 0.331_264 * g + 0.5 * b + 128.0;
            v_sum[block] += 0.5 * r - 0.418_688 * g - 0.081_312 * b + 128.0;
            count[block] += 1;
        }
    }
    let mut u = vec![0u8; uv_w * uv_h];
    let mut v = vec![0u8; uv_w * uv_h];
    for i in 0..u.len() {
        let n = count[i] as f32;
        u[i] = (u_sum[i] / n).round().clamp(0.0, 255.0) as u8;
        v[i] = (v_sum[i] / n).round().clamp(0.0, 255.0) as u8;
    }
    (y, u, v)
}

/// Check for the EBML magic plus the `webm` document type.
///
/// ```
/// assert!(!cprz_video::is_webm(&[]));
/// assert!(!cprz_video::is_webm(b"definitely not webm"));
/// ```
#[must_use]
pub fn is_webm(data: &[u8]) -> bool {
    let Some((id, size, header_len)) = read_element(data, 0) else {
        return false;
    };
    if id != 0x1A45_DFA3 {
        return false;
    }
    let Some(size) = size else {
        return false;
    };
    let Some(end) = header_len.checked_add(sized(size)) else {
        return false;
    };
    if end > data.len() {
        return false;
    }
    let mut pos = header_len;
    while pos < end {
        let Some((child, child_size, child_header)) = read_element(data, pos) else {
            return false;
        };
        let Some(child_end) = child_end(pos, child_header, child_size, end) else {
            return false;
        };
        if child == 0x4282 {
            return data.get(header_len_of(pos, child_header)..child_end)
                == Some(b"webm".as_slice());
        }
        pos = child_end;
    }
    false
}

/// Read dimensions and frame count back from an encoded container.
///
/// Parses the EBML structure (Tracks for dimensions and codec, Clusters
/// for the frame count) without decoding AV1 pixels.
pub fn probe(data: &[u8]) -> Result<VideoInfo, Error> {
    let invalid = |why: &str| Error::InvalidContainer(why.to_owned());
    let (id, size, header_len) =
        read_element(data, 0).ok_or_else(|| invalid("missing EBML header"))?;
    if id != 0x1A45_DFA3 {
        return Err(invalid("missing EBML header"));
    }
    let size = size.ok_or_else(|| invalid("EBML header has unknown size"))?;
    let ebml_end = header_len
        .checked_add(sized(size))
        .ok_or_else(|| invalid("EBML header overruns input"))?;
    if ebml_end > data.len() || !is_webm(data) {
        return Err(invalid("not a WebM container"));
    }
    let (segment, segment_size, segment_header) =
        read_element(data, ebml_end).ok_or_else(|| invalid("missing Segment"))?;
    if segment != 0x1853_8067 {
        return Err(invalid("missing Segment"));
    }
    let segment_end = match segment_size {
        Some(size) => ebml_end
            .checked_add(segment_header)
            .and_then(|start| start.checked_add(sized(size)))
            .ok_or_else(|| invalid("Segment overruns input"))?,
        None => data.len(),
    };
    if segment_end > data.len() {
        return Err(invalid("truncated Segment"));
    }

    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut av1 = false;
    let mut frames = 0usize;
    let mut pos = ebml_end + segment_header;
    while pos < segment_end {
        let (child, child_size, child_header) =
            read_element(data, pos).ok_or_else(|| invalid("truncated Segment child"))?;
        let child_end = child_end(pos, child_header, child_size, segment_end)
            .ok_or_else(|| invalid("element overruns its parent"))?;
        let body = &data[header_len_of(pos, child_header)..child_end];
        match child {
            0x1654_AE6B => {
                let (w, h, is_av1) = parse_tracks(body)?;
                width = w;
                height = h;
                av1 = is_av1;
            }
            0x1F43_B675 => {
                frames += count_cluster_frames(body)?;
            }
            _ => {}
        }
        pos = child_end;
    }
    if !av1 {
        return Err(invalid("no AV1 video track"));
    }
    let (Some(width), Some(height)) = (width, height) else {
        return Err(invalid("video track misses dimensions"));
    };
    if frames == 0 {
        return Err(invalid("container holds no frames"));
    }
    Ok(VideoInfo {
        width,
        height,
        frames,
    })
}

/// Parse the Tracks payload: dimensions and whether an AV1 track exists.
fn parse_tracks(body: &[u8]) -> Result<(Option<u32>, Option<u32>, bool), Error> {
    let invalid = |why: &str| Error::InvalidContainer(why.to_owned());
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut av1 = false;
    let mut pos = 0;
    while pos < body.len() {
        let (id, size, header) =
            read_element(body, pos).ok_or_else(|| invalid("truncated Tracks"))?;
        let end = child_end(pos, header, size, body.len())
            .ok_or_else(|| invalid("track entry overruns Tracks"))?;
        if id == 0xAE {
            let (w, h, is_av1) = parse_track_entry(&body[pos + header..end])?;
            if is_av1 {
                width = w.or(width);
                height = h.or(height);
                av1 = true;
            }
        }
        pos = end;
    }
    Ok((width, height, av1))
}

/// Parse one TrackEntry payload.
fn parse_track_entry(body: &[u8]) -> Result<(Option<u32>, Option<u32>, bool), Error> {
    let invalid = |why: &str| Error::InvalidContainer(why.to_owned());
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut av1 = false;
    let mut pos = 0;
    while pos < body.len() {
        let (id, size, header) =
            read_element(body, pos).ok_or_else(|| invalid("truncated TrackEntry"))?;
        let end = child_end(pos, header, size, body.len())
            .ok_or_else(|| invalid("track field overruns TrackEntry"))?;
        let field = &body[pos + header..end];
        match id {
            0x86 => {
                if field == b"V_AV1" {
                    av1 = true;
                }
            }
            0xE0 => {
                let (w, h) = parse_video(field)?;
                width = w.or(width);
                height = h.or(height);
            }
            _ => {}
        }
        pos = end;
    }
    Ok((width, height, av1))
}

/// Parse the Video payload of a track entry.
fn parse_video(body: &[u8]) -> Result<(Option<u32>, Option<u32>), Error> {
    let invalid = |why: &str| Error::InvalidContainer(why.to_owned());
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut pos = 0;
    while pos < body.len() {
        let (id, size, header) =
            read_element(body, pos).ok_or_else(|| invalid("truncated Video"))?;
        let end = child_end(pos, header, size, body.len())
            .ok_or_else(|| invalid("video field overruns Video"))?;
        let field = &body[pos + header..end];
        match id {
            0xB0 => width = Some(read_uint(field).ok_or_else(|| invalid("bad PixelWidth"))?),
            0xBA => height = Some(read_uint(field).ok_or_else(|| invalid("bad PixelHeight"))?),
            _ => {}
        }
        pos = end;
    }
    Ok((width, height))
}

/// Count the frames in one Cluster payload.
fn count_cluster_frames(body: &[u8]) -> Result<usize, Error> {
    let invalid = |why: &str| Error::InvalidContainer(why.to_owned());
    let mut frames = 0;
    let mut pos = 0;
    while pos < body.len() {
        let (id, size, header) =
            read_element(body, pos).ok_or_else(|| invalid("truncated Cluster"))?;
        let end = child_end(pos, header, size, body.len())
            .ok_or_else(|| invalid("block overruns Cluster"))?;
        match id {
            0xA3 => {
                // SimpleBlock: track vint, i16 timecode, flags, then OBU bytes.
                if end - (pos + header) < 4 {
                    return Err(invalid("truncated SimpleBlock"));
                }
                frames += 1;
            }
            0xA0 => {
                // BlockGroup: count the inner Block the same way.
                frames += count_group_block(&body[pos + header..end])?;
            }
            _ => {}
        }
        pos = end;
    }
    Ok(frames)
}

/// Count the single Block inside a BlockGroup payload.
fn count_group_block(body: &[u8]) -> Result<usize, Error> {
    let invalid = |why: &str| Error::InvalidContainer(why.to_owned());
    let mut frames = 0;
    let mut pos = 0;
    while pos < body.len() {
        let (id, size, header) =
            read_element(body, pos).ok_or_else(|| invalid("truncated BlockGroup"))?;
        let end = child_end(pos, header, size, body.len())
            .ok_or_else(|| invalid("block overruns BlockGroup"))?;
        if id == 0xA1 {
            if end - (pos + header) < 4 {
                return Err(invalid("truncated Block"));
            }
            frames += 1;
        }
        pos = end;
    }
    Ok(frames)
}

/// Read one EBML element: `(id, size, header length)`.
///
/// `size` is `None` for the unknown-size marker (all value bits set).
fn read_element(data: &[u8], pos: usize) -> Option<(u32, Option<u64>, usize)> {
    let first = *data.get(pos)?;
    let id_len = first.leading_zeros() as usize + 1;
    if !(1..=4).contains(&id_len) {
        return None;
    }
    let id_bytes = data.get(pos..pos + id_len)?;
    let mut id = 0u32;
    for byte in id_bytes {
        id = (id << 8) | u32::from(*byte);
    }
    let size_pos = pos + id_len;
    let size_first = *data.get(size_pos)?;
    let size_len = size_first.leading_zeros() as usize + 1;
    if !(1..=8).contains(&size_len) {
        return None;
    }
    let size_bytes = data.get(size_pos..size_pos + size_len)?;
    // Value mask for the first byte: the length marker consumes one bit,
    // so length 8 leaves no value bits there (computed in u16 to avoid a
    // shift-overflow panic on that length).
    let mask = ((0xFFu16 >> size_len) & 0xFF) as u8;
    let mut value = u64::from(size_bytes[0] & mask);
    for byte in &size_bytes[1..] {
        value = (value << 8) | u64::from(*byte);
    }
    let unknown = value == (1u64 << (7 * size_len)) - 1;
    Some((
        id,
        if unknown { None } else { Some(value) },
        id_len + size_len,
    ))
}

/// End offset of a child element inside a parent ending at `parent_end`.
fn child_end(pos: usize, header_len: usize, size: Option<u64>, parent_end: usize) -> Option<usize> {
    let start = pos.checked_add(header_len)?;
    let end = match size {
        Some(size) => start.checked_add(sized(size))?,
        None => parent_end,
    };
    if end > parent_end || end < start {
        return None;
    }
    Some(end)
}

/// Convert an EBML body size to `usize`, saturating hostile values so range
/// checks below reject them instead of truncating on 32-bit targets.
fn sized(size: u64) -> usize {
    usize::try_from(size).unwrap_or(usize::MAX)
}

/// Start offset of an element body, given its position and header length.
fn header_len_of(pos: usize, header_len: usize) -> usize {
    pos + header_len
}

/// Read a big-endian unsigned integer element body.
fn read_uint(body: &[u8]) -> Option<u32> {
    if body.is_empty() || body.len() > 4 {
        return None;
    }
    let mut value = 0u32;
    for byte in body {
        value = (value << 8) | u32::from(*byte);
    }
    Some(value)
}

// --- Minimal WebM muxer ----------------------------------------------------

/// Presentation timestamp of a frame in milliseconds.
fn timestamp_ms(index: usize, fps_num: u32, fps_den: u32) -> u64 {
    index as u64 * 1000 * u64::from(fps_den) / u64::from(fps_num)
}

/// Frames per Cluster: keeps SimpleBlock timecodes inside `i16` range.
fn cluster_len(fps_num: u32, fps_den: u32, total: usize) -> usize {
    let per_30s = 30u64 * u64::from(fps_num) / u64::from(fps_den);
    (per_30s.clamp(1, 240) as usize).min(total.max(1)).max(1)
}

/// Mux AV1 packets as SimpleBlocks into a browser-playable WebM file.
fn mux_webm(
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
    codec_private: &[u8],
    packets: &[(bool, Vec<u8>)],
) -> Vec<u8> {
    let mut ebml = Vec::new();
    uint_element(&mut ebml, &[0x42, 0x86], 1);
    uint_element(&mut ebml, &[0x42, 0xF7], 1);
    uint_element(&mut ebml, &[0x42, 0xF2], 4);
    uint_element(&mut ebml, &[0x42, 0xF3], 8);
    string_element(&mut ebml, &[0x42, 0x82], "webm");
    uint_element(&mut ebml, &[0x42, 0x87], 4);
    uint_element(&mut ebml, &[0x42, 0x85], 2);

    let total_ms = timestamp_ms(packets.len(), fps_num, fps_den);
    let mut info = Vec::new();
    uint_element(&mut info, &[0x2A, 0xD7, 0xB1], 1_000_000);
    string_element(&mut info, &[0x4D, 0x80], "cprz-video");
    string_element(&mut info, &[0x57, 0x41], "cprz-video");
    float_element(&mut info, &[0x44, 0x89], total_ms as f32);

    let mut video = Vec::new();
    uint_element(&mut video, &[0xB0], u64::from(width));
    uint_element(&mut video, &[0xBA], u64::from(height));
    let mut track = Vec::new();
    uint_element(&mut track, &[0xD7], 1);
    uint_element(&mut track, &[0x73, 0xC5], 1);
    uint_element(&mut track, &[0x83], 1);
    uint_element(&mut track, &[0x9C], 0);
    string_element(&mut track, &[0x86], "V_AV1");
    binary_element(&mut track, &[0x63, 0xA2], codec_private);
    master_element(&mut track, &[0xE0], &video);
    let mut tracks = Vec::new();
    master_element(&mut tracks, &[0xAE], &track);

    let mut segment = Vec::new();
    master_element(&mut segment, &[0x15, 0x49, 0xA9, 0x66], &info);
    master_element(&mut segment, &[0x16, 0x54, 0xAE, 0x6B], &tracks);

    let chunk = cluster_len(fps_num, fps_den, packets.len());
    let mut start = 0;
    while start < packets.len() {
        let end = (start + chunk).min(packets.len());
        let base = timestamp_ms(start, fps_num, fps_den);
        let mut cluster = Vec::new();
        uint_element(&mut cluster, &[0xE7], base);
        for (offset, (keyframe, data)) in packets[start..end].iter().enumerate() {
            let timecode = timestamp_ms(start + offset, fps_num, fps_den) - base;
            let flags = if *keyframe { 0x80 } else { 0x00 };
            let mut block = vec![0x81, (timecode >> 8) as u8, (timecode & 0xFF) as u8, flags];
            block.extend_from_slice(data);
            master_element(&mut cluster, &[0xA3], &block);
        }
        // Cluster with unknown size (single-byte 0xFF marker).
        segment.extend_from_slice(&[0x1F, 0x43, 0xB6, 0x75, 0xFF]);
        segment.extend_from_slice(&cluster);
        start = end;
    }

    let mut out = Vec::with_capacity(segment.len() + 64);
    master_element(&mut out, &[0x1A, 0x45, 0xDF, 0xA3], &ebml);
    // Segment with unknown size (single-byte marker; a longer marker would
    // leave stray bytes that parse as a phantom child spanning to EOF).
    out.extend_from_slice(&[0x18, 0x53, 0x80, 0x67, 0xFF]);
    out.extend_from_slice(&segment);
    out
}

/// Write an EBML size as the shortest vint (the all-ones value is reserved).
fn write_vint(out: &mut Vec<u8>, value: u64) {
    let mut len: usize = 1;
    while len < 8 && value >= (1u64 << (7 * len)) - 1 {
        len += 1;
    }
    let mut bytes = [0u8; 8];
    let mut rest = value;
    for i in (0..len).rev() {
        bytes[i] = (rest & 0xFF) as u8;
        rest >>= 8;
    }
    bytes[0] |= 0x80u8 >> (len - 1);
    out.extend_from_slice(&bytes[..len]);
}

/// Write an unsigned integer with the shortest big-endian encoding.
fn write_uint_value(out: &mut Vec<u8>, value: u64) {
    let len = (8 - value.leading_zeros() / 8).max(1) as usize;
    for i in (0..len).rev() {
        out.push((value >> (8 * i)) as u8);
    }
}

fn uint_element(out: &mut Vec<u8>, id: &[u8], value: u64) {
    let mut body = Vec::new();
    write_uint_value(&mut body, value);
    master_element(out, id, &body);
}

fn string_element(out: &mut Vec<u8>, id: &[u8], value: &str) {
    master_element(out, id, value.as_bytes());
}

fn binary_element(out: &mut Vec<u8>, id: &[u8], value: &[u8]) {
    master_element(out, id, value);
}

fn float_element(out: &mut Vec<u8>, id: &[u8], value: f32) {
    master_element(out, id, &value.to_be_bytes());
}

fn master_element(out: &mut Vec<u8>, id: &[u8], payload: &[u8]) {
    out.extend_from_slice(id);
    write_vint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;
