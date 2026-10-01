use super::*;

/// 256 KiB of periodic bytes, like a memory image with structure.
fn repetitive_blob() -> Vec<u8> {
    (0..262_144).map(|i| (i % 251) as u8).collect()
}

/// 640 KiB with a 64 KiB block repeated half a megabyte apart, so only
/// long-distance matching can link the copies.
fn distant_repeat_blob() -> Vec<u8> {
    let block: Vec<u8> = (0..65_536).map(|i| (i % 251) as u8).collect();
    let filler: Vec<u8> = (0..524_288).map(|i| ((i * 31 + 7) % 251) as u8).collect();
    let mut data = Vec::with_capacity(block.len() * 2 + filler.len());
    data.extend_from_slice(&block);
    data.extend_from_slice(&filler);
    data.extend_from_slice(&block);
    data
}

/// Deterministic pseudo-random bytes that barely compress.
fn incompressible_blob() -> Vec<u8> {
    let mut state: u32 = 0x1234_5678;
    (0..4096)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state & 0xff) as u8
        })
        .collect()
}

#[test]
fn header_value_is_stable() {
    assert_eq!(Codec::Zstd.as_byte(), 0x00);
    assert_eq!(Codec::from_byte(0x09), None);
    assert_eq!(codec_of(&[0x00]).unwrap(), Codec::Zstd);
}

#[test]
fn repetitive_blob_round_trip_with_ratio() {
    let input = repetitive_blob();
    let packet = compress(&input).unwrap();
    assert_eq!(decompress(&packet).unwrap(), input);
    // Periodic data must shrink to well under half its size.
    assert!(
        packet.len() < input.len() / 2,
        "packet {} vs input {}",
        packet.len(),
        input.len()
    );
}

#[test]
fn distant_repeat_blob_round_trip_with_ratio() {
    let input = distant_repeat_blob();
    let packet = compress(&input).unwrap();
    assert_eq!(decompress(&packet).unwrap(), input);
    assert!(
        packet.len() < input.len() / 2,
        "packet {} vs input {}",
        packet.len(),
        input.len()
    );
}

#[test]
fn incompressible_blob_round_trip() {
    let input = incompressible_blob();
    let packet = compress(&input).unwrap();
    assert_eq!(decompress(&packet).unwrap(), input);
}

#[test]
fn empty_input_round_trip() {
    let packet = compress(&[]).unwrap();
    assert_eq!(decompress(&packet).unwrap(), Vec::<u8>::new());
}

#[test]
fn compression_is_deterministic() {
    let input = repetitive_blob();
    assert_eq!(compress(&input).unwrap(), compress(&input).unwrap());
}

#[test]
fn malformed_packets_are_errors() {
    assert!(matches!(decompress(&[]), Err(Error::Empty)));
    assert!(matches!(codec_of(&[]), Err(Error::Empty)));
    assert!(matches!(
        decompress(&[0x09, 0x00]),
        Err(Error::UnknownCodec(0x09))
    ));
    // Truncated body behind a valid header.
    let packet = compress(&repetitive_blob()).unwrap();
    let truncated = &packet[..packet.len() / 2];
    assert!(decompress(truncated).is_err());
}
