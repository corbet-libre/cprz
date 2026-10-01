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

#[test]
fn malformed_structured_data_keeps_the_failed_codec_in_its_diagnostics() {
    use std::error::Error as _;
    for (packet, expected, cause) in [
        (vec![], "empty", false),
        (vec![255], "0xFF", false),
        (vec![HEADER_RAW], "bitcode", true),
        (vec![HEADER_ZSTD, 1, 2, 3], "zstd", true),
    ] {
        let error = unpack::<Profile>(&packet).unwrap_err();
        assert!(error.to_string().contains(expected));
        assert_eq!(error.source().is_some(), cause);
    }
}
