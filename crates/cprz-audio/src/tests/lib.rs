use super::*;

/// Two-second-friendly voice-like signal: mixed sines at the given rate.
fn tone(sample_rate_hz: u32, seconds: u32, amplitude: f32) -> Vec<i16> {
    let total = (sample_rate_hz * seconds) as usize;
    let mut out = Vec::with_capacity(total);
    for i in 0..total {
        let t = i as f32 / sample_rate_hz as f32;
        let wave = (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            + 0.5 * (2.0 * std::f32::consts::PI * 880.0 * t).sin();
        out.push((wave * amplitude) as i16);
    }
    out
}

fn rms(samples: &[i16]) -> f64 {
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / samples.len() as f64).sqrt()
}

#[test]
fn headers_form_a_valid_ogg_opus_stream() {
    let pcm = tone(16_000, 1, 8_000.0);
    let packet = encode_voice(&pcm, VoiceConfig::new(16_000, 12_000).unwrap()).unwrap();
    assert!(packet.starts_with(b"OggS"));

    let mut reader = PacketReader::new(Cursor::new(&packet[..]));
    let head = reader.read_packet().unwrap().unwrap();
    assert_eq!(&head.data[..8], b"OpusHead");
    assert_eq!(head.data[8], 1, "OpusHead version");
    assert_eq!(head.data[9], 1, "channel count is mono");
    assert_eq!(head.data.len(), 19, "mapping family 0 header length");
    let rate = u32::from_le_bytes(head.data[12..16].try_into().unwrap());
    assert_eq!(rate, 16_000);

    let tags = reader.read_packet().unwrap().unwrap();
    assert_eq!(&tags.data[..8], b"OpusTags");

    let audio = reader.read_packet().unwrap().unwrap();
    assert!(!audio.data.is_empty(), "first audio packet holds samples");
}

#[test]
fn round_trip_16k_preserves_length_and_energy() {
    let pcm = tone(16_000, 2, 8_000.0);
    let packet = encode_voice(&pcm, VoiceConfig::new(16_000, 12_000).unwrap()).unwrap();
    let back = decode_voice(&packet).unwrap();
    assert_eq!(back.sample_rate_hz, 16_000);
    assert_eq!(back.samples.len(), pcm.len());
    let (before, after) = (rms(&pcm), rms(&back.samples));
    assert!(
        after > before * 0.5 && after < before * 2.0,
        "rms {after:.0} vs {before:.0}"
    );
}

#[test]
fn round_trip_48k_preserves_length() {
    let pcm = tone(48_000, 1, 8_000.0);
    let packet = encode_voice(&pcm, VoiceConfig::new(48_000, 16_000).unwrap()).unwrap();

    let mut reader = PacketReader::new(Cursor::new(&packet[..]));
    let head = reader.read_packet().unwrap().unwrap();
    let rate = u32::from_le_bytes(head.data[12..16].try_into().unwrap());
    assert_eq!(rate, 48_000);

    let back = decode_voice(&packet).unwrap();
    assert_eq!(back.sample_rate_hz, 48_000);
    assert_eq!(back.samples.len(), pcm.len());
}

#[test]
fn bitrate_target_is_honoured() {
    let pcm = tone(16_000, 2, 8_000.0);
    let low = encode_voice(&pcm, VoiceConfig::new(16_000, 6_000).unwrap()).unwrap();
    let high = encode_voice(&pcm, VoiceConfig::new(16_000, 16_000).unwrap()).unwrap();
    // Constant bitrate: 6 kbps must stay clearly below 16 kbps.
    assert!(
        low.len() < high.len(),
        "6 kbps {} vs 16 kbps {}",
        low.len(),
        high.len()
    );
    // Two seconds at 6 kbps carry ~1500 payload bytes; at 16 kbps ~4000.
    // Both bounds leave wide room for Ogg framing overhead.
    assert!(
        (750..=3000).contains(&low.len()),
        "6 kbps packet has {} bytes",
        low.len()
    );
    assert!(
        (2000..=5500).contains(&high.len()),
        "16 kbps packet has {} bytes",
        high.len()
    );
    // Both still decode to the full signal.
    assert_eq!(decode_voice(&low).unwrap().samples.len(), pcm.len());
    assert_eq!(decode_voice(&high).unwrap().samples.len(), pcm.len());
}

#[test]
fn partial_final_frame_is_padded_with_silence() {
    let pcm = tone(16_000, 1, 8_000.0);
    let cut = pcm[..1000].to_vec();
    let packet = encode_voice(&cut, VoiceConfig::default()).unwrap();
    let back = decode_voice(&packet).unwrap();
    // Four 20 ms frames of 320 samples cover 1000 input samples.
    assert_eq!(back.samples.len(), 1280);
}

#[test]
fn encoding_is_deterministic() {
    let pcm = tone(16_000, 1, 8_000.0);
    let config = VoiceConfig::default();
    assert_eq!(
        encode_voice(&pcm, config).unwrap(),
        encode_voice(&pcm, config).unwrap()
    );
}

#[test]
fn invalid_input_is_rejected() {
    assert!(matches!(
        encode_voice(&[], VoiceConfig::default()),
        Err(Error::Empty)
    ));
    assert!(matches!(
        VoiceConfig::new(8_000, 12_000),
        Err(Error::UnsupportedSampleRate(8_000))
    ));
    assert!(matches!(
        VoiceConfig::new(16_000, 5_000),
        Err(Error::BitrateOutOfRange(5_000))
    ));
    assert!(matches!(decode_voice(&[]), Err(Error::Empty)));
    assert!(matches!(
        decode_voice(b"not an ogg stream...."),
        Err(Error::InvalidContainer)
    ));

    // A flipped byte inside the OpusHead magic breaks the stream.
    let pcm = tone(16_000, 1, 8_000.0);
    let mut packet = encode_voice(&pcm, VoiceConfig::default()).unwrap();
    packet[30] ^= 0xFF;
    assert!(matches!(
        decode_voice(&packet),
        Err(Error::InvalidContainer)
    ));
}
