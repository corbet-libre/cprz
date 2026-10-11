use super::*;

/// A realistic long text: a technical paragraph repeated like log output.
const PARAGRAPH: &str = "Packet switching splits each message into small blocks that travel \
    independently across the network and are reassembled at the destination. Sequence numbers \
    let the receiver restore the original order even when blocks arrive out of order. ";

/// A realistic short text: a commit message with context, around 300 bytes.
const SHORT_TEXT: &str = "Fix retransmit storm on link flap\n\nWhen the radio link flaps, queued \
    acknowledgements arrive late and the sender retransmits frames the peer already has. Back \
    off the retry timer while the link is unstable and drop duplicates by sequence number.";

fn long_text() -> String {
    PARAGRAPH.repeat(64)
}

#[test]
fn header_values_are_stable() {
    assert_eq!(Codec::Zstd.as_byte(), 0x00);
    assert_eq!(Codec::ZstdDict.as_byte(), 0x01);
    assert_eq!(Codec::Brotli.as_byte(), 0x02);
    assert_eq!(Codec::from_byte(0x09), None);
}

#[test]
fn long_text_round_trip_with_ratio() {
    let text = long_text();
    let packet = compress(&text, None).unwrap();
    assert!(matches!(
        codec_of(&packet).unwrap(),
        Codec::Zstd | Codec::ZstdDict | Codec::Brotli
    ));
    let back = decompress(&packet, None).unwrap();
    assert_eq!(back, text);
    // Repetitive text must shrink to well under half its size.
    assert!(
        packet.len() < text.len() / 2,
        "packet {} vs input {}",
        packet.len(),
        text.len()
    );
}

#[test]
fn short_text_round_trip_with_ratio() {
    let packet = compress(SHORT_TEXT, None).unwrap();
    let back = decompress(&packet, None).unwrap();
    assert_eq!(back, SHORT_TEXT);
    assert!(
        packet.len() < SHORT_TEXT.len(),
        "packet {} vs input {}",
        packet.len(),
        SHORT_TEXT.len()
    );
}

#[test]
fn empty_text_round_trip() {
    let packet = compress("", None).unwrap();
    assert_eq!(decompress(&packet, None).unwrap(), "");
}

#[test]
fn compression_is_deterministic() {
    let text = long_text();
    assert_eq!(
        compress(&text, None).unwrap(),
        compress(&text, None).unwrap()
    );
}

#[test]
fn dictionary_round_trip() {
    let dictionary = PARAGRAPH.repeat(8);
    let message = format!("{PARAGRAPH}Reported by node 7 after the nightly key rotation.");
    let dict_bytes = dictionary.as_bytes();
    let packet = compress(&message, Some(dict_bytes)).unwrap();
    assert_eq!(decompress(&packet, Some(dict_bytes)).unwrap(), message);
}

#[test]
fn missing_dictionary_is_an_error() {
    let dictionary = PARAGRAPH.repeat(8);
    let dict_bytes = dictionary.as_bytes();
    // Force a dictionary frame by using input the dictionary covers well;
    // if Brotli wins instead, this packet simply skips the check.
    let message = format!("{PARAGRAPH}Reported by node 7 after the nightly key rotation.");
    let packet = compress(&message, Some(dict_bytes)).unwrap();
    if codec_of(&packet).unwrap() == Codec::ZstdDict {
        assert!(matches!(
            decompress(&packet, None),
            Err(Error::MissingDictionary)
        ));
    }
}

#[test]
fn wrong_dictionary_does_not_recover_plaintext() {
    let dictionary = PARAGRAPH.repeat(8);
    let other = SHORT_TEXT.repeat(8);
    let message = format!("{PARAGRAPH}Reported by node 7 after the nightly key rotation.");
    let packet = compress(&message, Some(dictionary.as_bytes())).unwrap();
    if codec_of(&packet).unwrap() == Codec::ZstdDict {
        // A wrong dictionary must never reproduce the original text: zstd
        // either rejects the frame or decodes it to unrelated bytes.
        let decoded = decompress(&packet, Some(other.as_bytes())).ok();
        assert_ne!(decoded.as_deref(), Some(message.as_str()));
    }
}

#[test]
fn malformed_packets_are_errors() {
    assert!(matches!(decompress(&[], None), Err(Error::Empty)));
    assert!(matches!(codec_of(&[]), Err(Error::Empty)));
    assert!(matches!(
        decompress(&[0x09, 0x00], None),
        Err(Error::UnknownCodec(0x09))
    ));
    // Truncated body behind a valid header.
    let packet = compress(&long_text(), None).unwrap();
    let truncated = &packet[..packet.len() / 2];
    assert!(decompress(truncated, None).is_err());
}

#[test]
fn nested_codec_failures_retain_their_original_causes() {
    use std::error::Error as _;
    for (packet, dictionary, text, nested) in [
        (vec![], None, "empty", false),
        (vec![255], None, "0xFF", false),
        (vec![HEADER_ZSTD_DICT], None, "dictionary", false),
        (vec![HEADER_ZSTD_DICT], Some(&b""[..]), "dictionary", false),
        (vec![HEADER_ZSTD, 1, 2, 3], None, "zstd", true),
        (vec![HEADER_BROTLI, 255, 255], None, "brotli", true),
    ] {
        let error = decompress(&packet, dictionary).unwrap_err();
        assert!(error.to_string().contains(text), "{error}");
        assert_eq!(error.source().is_some(), nested);
    }
    for (codec, frame) in [
        (HEADER_ZSTD, encode_zstd(&[255, 254], None).unwrap()),
        (HEADER_BROTLI, encode_brotli(&[255, 254]).unwrap()),
    ] {
        let mut packet = vec![codec];
        packet.extend(frame);
        let error = decompress(&packet, None).unwrap_err();
        assert!(matches!(error, Error::Utf8(_)));
        assert!(error.to_string().contains("UTF-8"));
        assert!(error.source().is_some());
    }
}

#[test]
fn packet_selection_preserves_the_smallest_real_codec_frame() {
    // Public deterministic fixtures, with different alphabets and repetition.
    let numeric: String = (0u64..512)
        .map(|i| {
            format!(
                "{:016x}\n",
                i.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                    .rotate_left((i % 64) as u32)
            )
        })
        .collect();
    let unicode: String = (0..512)
        .map(|i| char::from_u32(0x400 + (i * 131) % 4096).unwrap())
        .collect();
    for text in [numeric, unicode, "0123456789abcdef".repeat(256)] {
        let zstd = encode_zstd(text.as_bytes(), None).unwrap();
        let brotli = encode_brotli(text.as_bytes()).unwrap();
        let packet = compress(&text, Some(&[])).unwrap();
        assert_eq!(packet.len(), 1 + zstd.len().min(brotli.len()));
        let expected = if brotli.len() < zstd.len() {
            Codec::Brotli
        } else {
            Codec::Zstd
        };
        assert_eq!(codec_of(&packet).unwrap(), expected);
        assert_eq!(decompress(&packet, None).unwrap(), text);
    }
}

#[test]
fn control_character_text_uses_the_smallest_real_frame_and_round_trips() {
    use std::hash::{DefaultHasher, Hasher};
    // Public deterministic data exercises generic text beyond prose/dictionaries.
    for length in [128usize, 2048, 16_384] {
        for alphabet in [4u8, 16, 64, 128] {
            let bytes: Vec<u8> = (0..length)
                .map(|i| {
                    let mut hash = DefaultHasher::new();
                    hash.write_usize(i);
                    (hash.finish() as u8) % alphabet
                })
                .collect();
            let text = String::from_utf8(bytes).unwrap();
            let zstd = encode_zstd(text.as_bytes(), None).unwrap();
            let brotli = encode_brotli(text.as_bytes()).unwrap();
            let packet = compress(&text, None).unwrap();
            assert_eq!(packet.len(), 1 + zstd.len().min(brotli.len()));
            let expected = if brotli.len() < zstd.len() {
                Codec::Brotli
            } else {
                Codec::Zstd
            };
            assert_eq!(codec_of(&packet).unwrap(), expected);
            assert_eq!(decompress(&packet, None).unwrap(), text);
        }
    }
}

#[test]
fn zstd_bomb_is_refused_under_the_limit() {
    let bomb = [
        &[Codec::Zstd.as_byte()][..],
        &encode_zstd(&vec![b'a'; 8 << 20], None).unwrap(),
    ]
    .concat();
    assert!(bomb.len() < 4096);
    assert!(matches!(
        decompress_with_limit(&bomb, None, 1 << 20),
        Err(Error::OutputLimit)
    ));
    assert_eq!(
        decompress_with_limit(&bomb, None, 8 << 20).unwrap().len(),
        8 << 20
    );
}

#[test]
fn brotli_bomb_is_refused_under_the_limit() {
    let bomb = [
        &[Codec::Brotli.as_byte()][..],
        &encode_brotli(&vec![b'a'; 8 << 20]).unwrap(),
    ]
    .concat();
    assert!(bomb.len() < 65_536);
    assert!(matches!(
        decompress_with_limit(&bomb, None, 1 << 20),
        Err(Error::OutputLimit)
    ));
    assert_eq!(
        decompress_with_limit(&bomb, None, 8 << 20).unwrap().len(),
        8 << 20
    );
}

#[test]
fn default_limit_accepts_ordinary_text() {
    let text = long_text();
    let packet = compress(&text, None).unwrap();
    assert_eq!(decompress(&packet, None).unwrap(), text);
}
