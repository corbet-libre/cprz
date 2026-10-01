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
