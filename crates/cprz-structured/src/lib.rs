//! Most-compact encoding for structured records and messages.
//!
//! Values are serialized with the [`bitcode`] crate's `serde` integration,
//! which packs booleans, small enums and bounded integers down to the bit
//! level, then compressed with zstd at the highest level when that shrinks
//! the payload further. A one-byte header records which form the packet
//! holds, so [`unpack`] needs no side information.
//!
//! # Example
//!
//! ```
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize, PartialEq, Debug)]
//! struct Profile {
//!     name: String,
//!     admin: bool,
//! }
//!
//! let value = Profile {
//!     name: String::from("ada"),
//!     admin: true,
//! };
//! let packet = cprz_structured::pack(&value);
//! let back: Profile = cprz_structured::unpack(&packet).unwrap();
//! assert_eq!(back, value);
//! ```

use std::io::Read;

/// Header byte marking a raw `bitcode` frame.
const HEADER_RAW: u8 = 0x00;

/// Header byte marking a zstd frame wrapping a `bitcode` frame.
const HEADER_ZSTD: u8 = 0x01;

/// The encoding a packet holds.
///
/// Matches the one-byte header documented in the crate README. Numeric values
/// are stable: never reuse a value for a different encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Codec {
    /// `bitcode` bytes stored directly.
    Raw,
    /// A zstd frame (highest level) wrapping `bitcode` bytes.
    Zstd,
}

impl Codec {
    /// Return the header byte identifying this encoding.
    #[must_use]
    pub fn as_byte(self) -> u8 {
        match self {
            Codec::Raw => HEADER_RAW,
            Codec::Zstd => HEADER_ZSTD,
        }
    }

    /// Parse a header byte into an encoding.
    #[must_use]
    pub fn from_byte(byte: u8) -> Option<Codec> {
        match byte {
            HEADER_RAW => Some(Codec::Raw),
            HEADER_ZSTD => Some(Codec::Zstd),
            _ => None,
        }
    }
}

/// Errors from packing or unpacking structured values.
#[derive(Debug)]
pub enum Error {
    /// The packet is empty; at least the one-byte header is required.
    Empty,
    /// The header byte names no known encoding.
    UnknownCodec(u8),
    /// The `bitcode` layer failed.
    Bitcode(bitcode::Error),
    /// The zstd layer failed.
    Zstd(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "empty packet"),
            Error::UnknownCodec(byte) => write!(f, "unknown codec header {byte:#04X}"),
            Error::Bitcode(source) => write!(f, "bitcode error: {source}"),
            Error::Zstd(source) => write!(f, "zstd error: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Bitcode(source) => Some(source),
            Error::Zstd(source) => Some(source),
            _ => None,
        }
    }
}

/// Pack a serializable value into the smallest available packet.
///
/// Serializes with `bitcode`, then also tries zstd at the highest level and
/// keeps the compressed form only when its body is strictly smaller (the
/// header costs one byte either way). Tiny records therefore stay raw while
/// larger or repetitive ones gain the zstd frame.
///
/// # Panics
///
/// Panics if the value uses `serde` shapes `bitcode` cannot represent (for
/// example untagged enums, `#[serde(tag = ...)]` enums or more than 256 enum
/// variants) or if compression runs out of memory. Plain structs and enums,
/// `Option`, sequences, maps and strings always work.
pub fn pack<T: serde::Serialize + ?Sized>(value: &T) -> Vec<u8> {
    let raw = bitcode::serialize(value).expect("value must be representable in bitcode");
    let level = *zstd::compression_level_range().end();
    let frame = zstd::bulk::compress(&raw, level).expect("zstd must compress in-memory bytes");
    if frame.len() < raw.len() {
        let mut packet = Vec::with_capacity(1 + frame.len());
        packet.push(Codec::Zstd.as_byte());
        packet.extend_from_slice(&frame);
        packet
    } else {
        let mut packet = Vec::with_capacity(1 + raw.len());
        packet.push(Codec::Raw.as_byte());
        packet.extend_from_slice(&raw);
        packet
    }
}

/// Report which encoding a packet uses, reading only its header byte.
pub fn codec_of(packet: &[u8]) -> Result<Codec, Error> {
    let header = packet.first().ok_or(Error::Empty)?;
    Codec::from_byte(*header).ok_or(Error::UnknownCodec(*header))
}

/// Unpack a value produced by [`pack`] back to its original form.
pub fn unpack<T: serde::de::DeserializeOwned>(packet: &[u8]) -> Result<T, Error> {
    match codec_of(packet)? {
        Codec::Raw => bitcode::deserialize(&packet[1..]).map_err(Error::Bitcode),
        Codec::Zstd => {
            let mut decoder = zstd::Decoder::new(&packet[1..]).map_err(Error::Zstd)?;
            let mut raw = Vec::new();
            decoder.read_to_end(&mut raw).map_err(Error::Zstd)?;
            bitcode::deserialize(&raw).map_err(Error::Bitcode)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    /// Small enum, like a role or status field in a real record.
    #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
    enum Role {
        Admin,
        Editor,
        Viewer,
    }

    /// Profile-like record: booleans, a small enum, bounded integers, short
    /// strings and an optional field, mirroring the records this crate
    /// targets. Plain serde shapes only: no untagged enums, no
    /// `skip_serializing_if`, which `bitcode` cannot represent.
    #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct Profile {
        name: String,
        active: bool,
        admin: bool,
        role: Role,
        age: u8,
        score: u32,
        nickname: Option<String>,
    }

    fn sample_profile() -> Profile {
        Profile {
            name: String::from("ada"),
            active: true,
            admin: false,
            role: Role::Editor,
            age: 36,
            score: 1500,
            nickname: Some(String::from("lovelace")),
        }
    }

    fn json_len<T: serde::Serialize + ?Sized>(value: &T) -> usize {
        serde_json::to_vec(value).unwrap().len()
    }

    #[test]
    fn header_values_are_stable() {
        assert_eq!(Codec::Raw.as_byte(), 0x00);
        assert_eq!(Codec::Zstd.as_byte(), 0x01);
        assert_eq!(Codec::from_byte(0x09), None);
        assert_eq!(codec_of(&[0x00]).unwrap(), Codec::Raw);
        assert_eq!(codec_of(&[0x01]).unwrap(), Codec::Zstd);
    }

    #[test]
    fn tiny_profile_uses_raw_codec_and_round_trips() {
        let value = sample_profile();
        let packet = pack(&value);
        assert_eq!(codec_of(&packet).unwrap(), Codec::Raw);
        assert_eq!(unpack::<Profile>(&packet).unwrap(), value);
    }

    #[test]
    fn packed_profile_is_smaller_than_json() {
        let value = sample_profile();
        let packet = pack(&value);
        let json = json_len(&value);
        assert!(
            packet.len() < json,
            "packet {} vs json {json}",
            packet.len()
        );
        // Field names dominate the JSON form, so the packed record must be
        // far smaller, not just marginally so.
        assert!(
            packet.len() * 2 <= json,
            "packet {} vs json {json}",
            packet.len()
        );
    }

    #[test]
    fn all_roles_and_options_round_trip() {
        let mut admin = sample_profile();
        admin.role = Role::Admin;
        admin.nickname = None;
        let mut viewer = sample_profile();
        viewer.role = Role::Viewer;
        viewer.name = String::new();
        viewer.age = u8::MAX;
        viewer.score = u32::MAX;
        viewer.nickname = Some(String::new());
        for value in [admin, viewer] {
            let packet = pack(&value);
            assert_eq!(unpack::<Profile>(&packet).unwrap(), value);
            assert!(packet.len() < json_len(&value));
        }
    }

    #[test]
    fn batch_of_profiles_uses_zstd_and_round_trips() {
        let batch = vec![sample_profile(); 200];
        let packet = pack(&batch);
        assert_eq!(codec_of(&packet).unwrap(), Codec::Zstd);
        assert_eq!(unpack::<Vec<Profile>>(&packet).unwrap(), batch);
        // The zstd frame must beat both the raw bitcode form and JSON.
        let raw = bitcode::serialize(&batch).unwrap();
        assert!(
            packet.len() < 1 + raw.len(),
            "packet {} vs raw {}",
            packet.len(),
            raw.len()
        );
        assert!(
            packet.len() < json_len(&batch),
            "packet {} vs json {}",
            packet.len(),
            json_len(&batch)
        );
    }

    #[test]
    fn primitives_and_collections_round_trip() {
        let number = 1500_u32;
        assert_eq!(unpack::<u32>(&pack(&number)).unwrap(), number);
        let text = String::from("hello");
        assert_eq!(unpack::<String>(&pack(&text)).unwrap(), text);
        let nothing: Option<String> = None;
        assert_eq!(unpack::<Option<String>>(&pack(&nothing)).unwrap(), nothing);
        let small = vec![sample_profile(); 3];
        assert_eq!(unpack::<Vec<Profile>>(&pack(&small)).unwrap(), small);
    }

    #[test]
    fn packing_is_deterministic() {
        let value = sample_profile();
        assert_eq!(pack(&value), pack(&value));
        let batch = vec![sample_profile(); 200];
        assert_eq!(pack(&batch), pack(&batch));
    }

    #[test]
    fn malformed_packets_are_errors() {
        assert!(matches!(unpack::<Profile>(&[]), Err(Error::Empty)));
        assert!(matches!(codec_of(&[]), Err(Error::Empty)));
        assert!(matches!(
            unpack::<Profile>(&[0x09, 0x00]),
            Err(Error::UnknownCodec(0x09))
        ));
        // Raw body missing entirely behind a valid header.
        assert!(unpack::<Profile>(&[0x00]).is_err());
        // Truncated raw body behind a valid header.
        let packet = pack(&sample_profile());
        let truncated = &packet[..packet.len() - 1];
        assert!(unpack::<Profile>(truncated).is_err());
        // Truncated zstd body behind a valid header.
        let zstd_packet = pack(&vec![sample_profile(); 200]);
        assert_eq!(codec_of(&zstd_packet).unwrap(), Codec::Zstd);
        let truncated = &zstd_packet[..zstd_packet.len() / 2];
        assert!(unpack::<Vec<Profile>>(truncated).is_err());
    }
}
