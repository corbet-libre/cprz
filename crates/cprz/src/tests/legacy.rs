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
