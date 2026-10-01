//! Voice and sound: Opus at low bitrates in an Ogg container.
//!
//! Mono PCM samples are encoded with a pure-Rust Opus encoder (see the crate
//! README for the survey behind that choice) at a caller-chosen voice bitrate
//! (6–16 kbps) and wrapped in a minimal Ogg Opus stream (RFC 7845) so the
//! output plays in any Ogg Opus capable player. Decoding needs no side
//! information: the sample rate travels in the `OpusHead` packet.
//!
//! Encoding uses constant bitrate and the highest encoder effort, favouring a
//! predictable size over raw quality. Audio is lossy, so decoding returns
//! samples that sound like the input rather than the exact input bytes; the
//! sample count is preserved up to padding of the final 20 ms frame with
//! silence.
//!
//! # Example
//!
//! ```
//! use cprz_audio::{VoiceConfig, decode_voice, encode_voice};
//!
//! // One second of middle-A at 16 kHz.
//! let rate = 16_000;
//! let pcm: Vec<i16> = (0..rate)
//!     .map(|i| {
//!         let phase = i as f32 * 440.0 * std::f32::consts::TAU / rate as f32;
//!         (phase.sin() * 10_000.0) as i16
//!     })
//!     .collect();
//! let config = VoiceConfig::new(rate, 12_000)?;
//! let packet = encode_voice(&pcm, config)?;
//! let back = decode_voice(&packet)?;
//! assert_eq!(back.sample_rate_hz, rate);
//! assert_eq!(back.samples.len(), pcm.len());
//! # Ok::<(), cprz_audio::Error>(())
//! ```

use ogg::{PacketReader, PacketWriteEndInfo, PacketWriter};
use opus_rs::{Application, OpusDecoder, OpusEncoder};
use std::io::Cursor;

/// Lowest voice bitrate this crate encodes, in bits per second.
pub const MIN_BITRATE_BPS: u32 = 6_000;
/// Highest voice bitrate this crate encodes, in bits per second.
pub const MAX_BITRATE_BPS: u32 = 16_000;

/// Sample rates accepted for voice, in Hz.
pub const SUPPORTED_SAMPLE_RATES: [u32; 2] = [16_000, 48_000];

/// Audio frame length in milliseconds; Opus supports 2.5–60 ms.
const FRAME_MS: u32 = 20;
/// Largest Opus frame in samples per channel (20 ms at 48 kHz).
const MAX_FRAME_SAMPLES: usize = 960;
/// Largest decode window in samples per channel (120 ms at 48 kHz).
const MAX_DECODE_SAMPLES: usize = 5760;
/// Scratch buffer per encoded frame; RFC 6716 caps a packet at 1276 bytes.
const ENCODE_SCRATCH: usize = 1500;
/// Fixed Ogg stream serial number, so encoding is deterministic.
const STREAM_SERIAL: u32 = 0x7A72_7063;
/// Vendor string written into the `OpusTags` packet.
const VENDOR: &[u8] = b"cprz-audio";

/// Voice encoder settings: sample rate and target bitrate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VoiceConfig {
    /// Input sample rate in Hz; one of [`SUPPORTED_SAMPLE_RATES`].
    sample_rate_hz: u32,
    /// Target constant bitrate in bits per second, 6–16 kbps.
    bitrate_bps: u32,
}

impl VoiceConfig {
    /// Build a voice configuration, validating rate and bitrate.
    ///
    /// Accepts 16 or 48 kHz mono and a bitrate of 6–16 kbps.
    pub fn new(sample_rate_hz: u32, bitrate_bps: u32) -> Result<Self, Error> {
        if !SUPPORTED_SAMPLE_RATES.contains(&sample_rate_hz) {
            return Err(Error::UnsupportedSampleRate(sample_rate_hz));
        }
        if !(MIN_BITRATE_BPS..=MAX_BITRATE_BPS).contains(&bitrate_bps) {
            return Err(Error::BitrateOutOfRange(bitrate_bps));
        }
        Ok(Self {
            sample_rate_hz,
            bitrate_bps,
        })
    }

    /// Return the configured sample rate in Hz.
    #[must_use]
    pub fn sample_rate_hz(self) -> u32 {
        self.sample_rate_hz
    }

    /// Return the configured target bitrate in bits per second.
    #[must_use]
    pub fn bitrate_bps(self) -> u32 {
        self.bitrate_bps
    }
}

impl Default for VoiceConfig {
    /// Default voice settings: 16 kHz mono at 12 kbps.
    fn default() -> Self {
        Self {
            sample_rate_hz: 16_000,
            bitrate_bps: 12_000,
        }
    }
}

/// Decoded mono voice: the sample rate plus the PCM samples.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoiceData {
    /// Sample rate of `samples` in Hz; one of [`SUPPORTED_SAMPLE_RATES`].
    pub sample_rate_hz: u32,
    /// Mono PCM samples at `sample_rate_hz`.
    pub samples: Vec<i16>,
}

/// Errors from encoding or decoding voice.
#[derive(Debug)]
pub enum Error {
    /// The input held no samples, or the packet held no bytes.
    Empty,
    /// The sample rate is not 16 or 48 kHz.
    UnsupportedSampleRate(u32),
    /// The bitrate is outside 6–16 kbps.
    BitrateOutOfRange(u32),
    /// The bytes are not an Ogg Opus stream this crate wrote or accepts.
    InvalidContainer,
    /// The Opus encoder or decoder rejected the data.
    Codec(&'static str),
    /// Writing the Ogg container failed.
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "no audio samples"),
            Error::UnsupportedSampleRate(rate) => {
                write!(f, "unsupported sample rate {rate} Hz (need 16000 or 48000)")
            }
            Error::BitrateOutOfRange(rate) => {
                write!(
                    f,
                    "bitrate {rate} bps outside {MIN_BITRATE_BPS}..={MAX_BITRATE_BPS}"
                )
            }
            Error::InvalidContainer => write!(f, "not an Ogg Opus stream"),
            Error::Codec(reason) => write!(f, "opus codec error: {reason}"),
            Error::Io(source) => write!(f, "ogg container I/O error: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(source) => Some(source),
            _ => None,
        }
    }
}

/// Samples per 20 ms frame at the given rate.
fn frame_size(sample_rate_hz: u32) -> usize {
    (sample_rate_hz / (1000 / FRAME_MS)) as usize
}

/// Build the 19-byte `OpusHead` packet for mono (RFC 7845, mapping family 0).
fn opus_head(sample_rate_hz: u32) -> [u8; 19] {
    let mut head = [0u8; 19];
    head[0..8].copy_from_slice(b"OpusHead");
    head[8] = 1; // version
    head[9] = 1; // channel count: mono
    // Bytes 10..12 stay zero: pre-skip (encoder delay is not trimmed).
    head[12..16].copy_from_slice(&sample_rate_hz.to_le_bytes());
    // Bytes 16..18 stay zero: output gain. Byte 18 stays zero: mapping family.
    head
}

/// Build the `OpusTags` packet: magic, vendor string, zero user comments.
fn opus_tags() -> Vec<u8> {
    let mut tags = Vec::with_capacity(8 + 4 + VENDOR.len() + 4);
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
    tags.extend_from_slice(VENDOR);
    tags.extend_from_slice(&0u32.to_le_bytes());
    tags
}

/// Parse `OpusHead`, returning the sample rate and the pre-skip in 48 kHz samples.
fn parse_opus_head(data: &[u8]) -> Result<(u32, u16), Error> {
    if data.len() < 19 || &data[..8] != b"OpusHead" || data[8] != 1 || data[9] != 1 || data[18] != 0
    {
        return Err(Error::InvalidContainer);
    }
    let pre_skip = u16::from_le_bytes([data[10], data[11]]);
    let rate = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    if !SUPPORTED_SAMPLE_RATES.contains(&rate) {
        return Err(Error::UnsupportedSampleRate(rate));
    }
    Ok((rate, pre_skip))
}

/// Check the `OpusTags` magic; comments are accepted and ignored.
fn parse_opus_tags(data: &[u8]) -> Result<(), Error> {
    if data.len() < 8 || &data[..8] != b"OpusTags" {
        return Err(Error::InvalidContainer);
    }
    Ok(())
}

/// Encode mono PCM samples as an Ogg Opus stream.
///
/// The input holds interleaved mono `i16` samples at the configured rate; a
/// trailing partial 20 ms frame is padded with silence. The output is a
/// complete Ogg Opus stream (headers plus audio pages) playable elsewhere.
pub fn encode_voice(pcm: &[i16], config: VoiceConfig) -> Result<Vec<u8>, Error> {
    if pcm.is_empty() {
        return Err(Error::Empty);
    }
    let rate = config.sample_rate_hz;
    let frame = frame_size(rate);
    let mut encoder = OpusEncoder::new(rate as i32, 1, Application::Voip).map_err(Error::Codec)?;
    // Constant bitrate at the highest effort: predictable sizes, smallest output.
    encoder.bitrate_bps = config.bitrate_bps as i32;
    encoder.complexity = 10;
    encoder.use_cbr = true;

    // The Ogg writer borrows packet bytes, so every buffer outlives it: all
    // frames are encoded first, then framed.
    let head = opus_head(rate);
    let tags = opus_tags();
    let frames = pcm.len().div_ceil(frame);
    let mut packets: Vec<Vec<u8>> = Vec::with_capacity(frames);
    {
        let mut scratch = [0u8; ENCODE_SCRATCH];
        let mut staging = [0i16; MAX_FRAME_SAMPLES];
        for chunk in pcm.chunks(frame) {
            staging[..chunk.len()].copy_from_slice(chunk);
            staging[chunk.len()..frame].fill(0);
            let used = encoder
                .encode_i16(&staging[..frame], frame, &mut scratch)
                .map_err(Error::Codec)?;
            packets.push(scratch[..used].to_vec());
        }
    }

    let mut writer = PacketWriter::new(Vec::new());
    writer
        .write_packet(&head[..], STREAM_SERIAL, PacketWriteEndInfo::EndPage, 0)
        .map_err(Error::Io)?;
    writer
        .write_packet(&tags[..], STREAM_SERIAL, PacketWriteEndInfo::EndPage, 0)
        .map_err(Error::Io)?;

    // Granule positions count decoded samples at 48 kHz (RFC 7845).
    let step = frame as u64 * 48_000 / u64::from(rate);
    let mut granule: u64 = 0;
    for (index, audio) in packets.iter().enumerate() {
        granule += step;
        let end = if index + 1 == frames {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        writer
            .write_packet(&audio[..], STREAM_SERIAL, end, granule)
            .map_err(Error::Io)?;
    }
    Ok(writer.into_inner())
}

/// Decode an Ogg Opus stream from [`encode_voice`] back to mono PCM.
///
/// Reads the sample rate from `OpusHead`, so no side information is needed.
/// The result holds one sample per input sample (up to final-frame padding);
/// values differ slightly from the input because Opus is lossy.
pub fn decode_voice(packet: &[u8]) -> Result<VoiceData, Error> {
    if packet.is_empty() {
        return Err(Error::Empty);
    }
    let mut reader = PacketReader::new(Cursor::new(packet));
    let head = reader
        .read_packet()
        .map_err(|_| Error::InvalidContainer)?
        .ok_or(Error::InvalidContainer)?;
    let tags = reader
        .read_packet()
        .map_err(|_| Error::InvalidContainer)?
        .ok_or(Error::InvalidContainer)?;
    let (rate, pre_skip) = parse_opus_head(&head.data)?;
    parse_opus_tags(&tags.data)?;

    let mut decoder = OpusDecoder::new(rate as i32, 1).map_err(Error::Codec)?;
    let mut frame = [0f32; MAX_DECODE_SAMPLES];
    let mut samples = Vec::new();
    loop {
        let next = reader.read_packet().map_err(|_| Error::InvalidContainer)?;
        let Some(audio) = next else {
            break;
        };
        if audio.data.is_empty() {
            return Err(Error::InvalidContainer);
        }
        let decoded = decoder
            .decode(&audio.data, MAX_DECODE_SAMPLES, &mut frame)
            .map_err(Error::Codec)?;
        samples.reserve(decoded);
        for &sample in &frame[..decoded] {
            let scaled = (sample.clamp(-1.0, 1.0) * 32768.0).round() as i32;
            samples.push(scaled.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
        }
    }
    if samples.is_empty() {
        return Err(Error::InvalidContainer);
    }
    // Drop the encoder-delay samples the header asks us to skip (zero here).
    let skip = (u32::from(pre_skip) * rate / 48_000) as usize;
    if skip < samples.len() {
        samples.drain(..skip);
    }
    Ok(VoiceData {
        sample_rate_hz: rate,
        samples,
    })
}

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;
