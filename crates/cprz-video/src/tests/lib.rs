use super::*;

#[test]
fn malformed_ebml_lengths_are_bounded_before_slicing() {
    for input in [vec![], vec![0], vec![8], vec![0xA1, 0], vec![0xA1, 0x40]] {
        assert!(read_element(&input, 0).is_none());
    }
    assert_eq!(read_uint(&[]), None);
    assert_eq!(read_uint(&[1; 5]), None);
    assert_eq!(read_uint(&[0, 0, 1, 0]), Some(256));
    assert_eq!(child_end(usize::MAX, 1, None, usize::MAX), None);
    assert_eq!(child_end(10, 1, None, 9), None);
    assert_eq!(child_end(0, 2, Some(10), 5), None);
    assert_eq!(child_end(1, 2, None, 5), Some(5));
    for value in [0, 126, 127, 16_382, 16_383, (1u64 << 56) - 2] {
        let mut bytes = vec![0xA1];
        write_vint(&mut bytes, value);
        assert_eq!(
            read_element(&bytes, 0),
            Some((0xA1, Some(value), bytes.len()))
        );
    }
    for bytes in [
        vec![0xEC, 0x80],
        vec![0x1A, 0x45, 0xDF, 0xA3, 0xFF],
        vec![0x1A, 0x45, 0xDF, 0xA3, 0x85, 0],
    ] {
        assert!(!is_webm(&bytes));
        assert!(matches!(probe(&bytes), Err(Error::InvalidContainer(_))));
    }
}

#[test]
fn nested_metadata_parsers_reject_truncation_and_parent_overruns() {
    for body in [vec![0], vec![0xAE, 0x84, 0]] {
        assert!(parse_tracks(&body).is_err());
        assert!(parse_track_entry(&body).is_err());
        assert!(parse_video(&body).is_err());
        assert!(count_cluster_frames(&body).is_err());
        assert!(count_group_block(&body).is_err());
    }
    assert!(count_cluster_frames(&[0xA3, 0x83, 0, 0, 0]).is_err());
    assert!(count_group_block(&[0xA1, 0x83, 0, 0, 0]).is_err());
    assert_eq!(count_group_block(&[0xEC, 0x80]).unwrap(), 0);
    assert_eq!(parse_tracks(&[0xEC, 0x80]).unwrap(), (None, None, false));
    assert_eq!(
        parse_track_entry(&[0x86, 0x83, b'V', b'P', b'9']).unwrap(),
        (None, None, false)
    );
    assert_eq!(parse_video(&[0xEC, 0x80]).unwrap(), (None, None));
    assert!(parse_video(&[0xB0, 0x80]).is_err());
    assert!(parse_video(&[0xBA, 0x85, 0, 0, 0, 0, 1]).is_err());
}

#[test]
fn a_real_av1_packet_is_counted_in_both_supported_webm_block_forms() {
    fn body(data: &[u8], wanted: u32) -> &[u8] {
        let mut offset = 0;
        while offset < data.len() {
            let (id, size, header) = read_element(data, offset).unwrap();
            let end = child_end(offset, header, size, data.len()).unwrap();
            if id == wanted {
                return &data[offset + header..end];
            }
            offset = end;
        }
        panic!("encoded fixture is missing a required element")
    }
    let output = encode_rgb(&rgb_frames(32, 32, 1), 32, 32, 30, 1, &quality_options(120)).unwrap();
    let segment = body(&output.data, 0x1853_8067);
    let cluster = body(segment, 0x1F43_B675);
    let packet = body(cluster, 0xA3);
    let mut group = Vec::new();
    master_element(&mut group, &[0xA1], packet);
    master_element(&mut group, &[0xEC], &[]);
    assert_eq!(count_group_block(&group).unwrap(), 1);
    let mut replacement = Vec::new();
    master_element(&mut replacement, &[0xA0], &group);
    assert_eq!(count_cluster_frames(&replacement).unwrap(), 1);
    assert_eq!(count_cluster_frames(cluster).unwrap(), 1);
    let (_, ebml_size, ebml_header) = read_element(&output.data, 0).unwrap();
    let mut finite = output.data[..ebml_header + sized(ebml_size.unwrap())].to_vec();
    master_element(&mut finite, &[0x18, 0x53, 0x80, 0x67], segment);
    assert_eq!(probe(&finite).unwrap(), probe(&output.data).unwrap());
}

#[test]
fn independent_invalid_frame_fields_identify_the_actual_fault() {
    for (width, height) in [(64, 0), (MAX_DIMENSION + 1, 64), (64, MAX_DIMENSION + 1)] {
        let error = encode(
            &Clip::new(width, height, 30, 1, vec![]),
            &quality_options(120),
        )
        .unwrap_err();
        assert!(matches!(error, Error::InvalidDimensions { .. }));
        assert!(error.to_string().contains(&format!("{width}x{height}")));
    }
    let error = encode(&Clip::new(64, 64, 30, 0, vec![]), &quality_options(120)).unwrap_err();
    assert!(matches!(error, Error::InvalidFps { num: 30, den: 0 }));
    assert!(error.to_string().contains("30/0"));
    for (u, v) in [(vec![0; 3], vec![0; 4]), (vec![0; 4], vec![0; 3])] {
        let error = sample_frame(
            &RawFrame::Yuv420 {
                y: &[0; 16],
                u: &u,
                v: &v,
            },
            7,
            4,
            4,
            2,
            2,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            Error::FrameSizeMismatch {
                index: 7,
                expected: 4,
                actual: 3
            }
        ));
        assert!(error.to_string().contains('7'));
    }
}

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const FRAMES: usize = 4;

/// Deterministic textured gradient; `seed` varies the frame content.
fn gradient_rgb(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let r = x
                .wrapping_mul(13)
                .wrapping_add(y.wrapping_mul(29))
                .wrapping_add(u32::from(seed).wrapping_mul(7))
                ^ x.wrapping_mul(y);
            let g = x
                .wrapping_mul(7)
                .wrapping_add(y.wrapping_mul(11))
                .wrapping_add(u32::from(seed).wrapping_mul(13));
            let b = x
                .wrapping_mul(x)
                .wrapping_add(y.wrapping_mul(y))
                .wrapping_add(u32::from(seed));
            out.push((r % 256) as u8);
            out.push((g % 256) as u8);
            out.push((b % 256) as u8);
        }
    }
    out
}

fn rgb_frames(width: u32, height: u32, count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| gradient_rgb(width, height, i as u8))
        .collect()
}

fn quality_options(quantizer: usize) -> EncodeOptions {
    EncodeOptions::quality(quantizer, SPEED_MAX)
}

#[test]
fn valid_webm_header() {
    let frames = rgb_frames(WIDTH, HEIGHT, FRAMES);
    let out = encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(120)).expect("encodes");
    assert!(is_webm(&out.data));
    let info = probe(&out.data).expect("probes");
    assert_eq!(
        (info.width, info.height, info.frames),
        (WIDTH, HEIGHT, FRAMES)
    );
    assert_eq!(out.info(), info);
    assert_eq!(out.dimensions(), (WIDTH, HEIGHT));
    assert_eq!(out.frame_count(), FRAMES);
    assert_eq!(out.byte_size(), out.data.len());
    assert!(out.byte_size() > 100);
}

#[test]
fn frame_count_and_dimensions_preserved() {
    let frames = rgb_frames(64, 48, 5);
    let out = encode_rgb(&frames, 64, 48, 24, 1, &quality_options(120)).expect("encodes");
    let info = probe(&out.data).expect("probes");
    assert_eq!((info.width, info.height, info.frames), (64, 48, 5));
}

#[test]
fn quality_affects_size() {
    let frames = rgb_frames(WIDTH, HEIGHT, FRAMES);
    let high = encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(40)).expect("encodes");
    let low = encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(220)).expect("encodes");
    assert!(is_webm(&high.data) && is_webm(&low.data));
    assert!(
        low.byte_size() < high.byte_size(),
        "quantizer 220 ({} bytes) should be smaller than quantizer 40 ({} bytes)",
        low.byte_size(),
        high.byte_size()
    );
}

#[test]
fn bitrate_affects_size() {
    // Sixteen frames give single-pass rate control room to engage; a
    // four-frame clip encodes identically at any bitrate target.
    let frames = rgb_frames(WIDTH, HEIGHT, 16);
    let low = encode_rgb(
        &frames,
        WIDTH,
        HEIGHT,
        30,
        1,
        &EncodeOptions::bitrate(64, SPEED_MAX),
    )
    .expect("encodes");
    let high = encode_rgb(
        &frames,
        WIDTH,
        HEIGHT,
        30,
        1,
        &EncodeOptions::bitrate(2048, SPEED_MAX),
    )
    .expect("encodes");
    assert!(is_webm(&low.data) && is_webm(&high.data));
    let low_info = probe(&low.data).expect("probes");
    let high_info = probe(&high.data).expect("probes");
    assert_eq!(
        (low_info.width, low_info.height, low_info.frames),
        (WIDTH, HEIGHT, 16)
    );
    assert_eq!(
        (high_info.width, high_info.height, high_info.frames),
        (WIDTH, HEIGHT, 16)
    );
    assert!(
        low.byte_size() < high.byte_size(),
        "64 kbit/s ({} bytes) should be smaller than 2048 kbit/s ({} bytes)",
        low.byte_size(),
        high.byte_size()
    );
}

#[test]
fn slower_speed_compresses_at_least_as_well() {
    let frames = rgb_frames(WIDTH, HEIGHT, FRAMES);
    let slow = encode_rgb(
        &frames,
        WIDTH,
        HEIGHT,
        30,
        1,
        &EncodeOptions::quality(120, DEFAULT_SPEED),
    )
    .expect("encodes");
    let fast = encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(120)).expect("encodes");
    assert!(is_webm(&slow.data) && is_webm(&fast.data));
    assert!(
        slow.byte_size() <= fast.byte_size(),
        "speed {DEFAULT_SPEED} ({} bytes) should not be larger than speed {SPEED_MAX} ({} bytes)",
        slow.byte_size(),
        fast.byte_size()
    );
}

#[test]
fn yuv_input_preserves_shape() {
    let w = 64usize;
    let h = 48usize;
    let frames: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = (0..3)
        .map(|s| {
            let (y, u, v) = rgb_to_yuv420(&gradient_rgb(64, 48, s), w, h);
            (y, u, v)
        })
        .collect();
    let raw: Vec<RawFrame<'_>> = frames
        .iter()
        .map(|(y, u, v)| RawFrame::Yuv420 {
            y: y.as_slice(),
            u: u.as_slice(),
            v: v.as_slice(),
        })
        .collect();
    let clip = Clip::new(64, 48, 30, 1, raw);
    assert_eq!(clip.frame_count(), 3);
    let out = encode(&clip, &quality_options(120)).expect("encodes");
    let info = probe(&out.data).expect("probes");
    assert_eq!((info.width, info.height, info.frames), (64, 48, 3));
}

#[test]
fn default_options_encode() {
    let frames = rgb_frames(32, 32, 2);
    let clip = Clip::new(
        32,
        32,
        30,
        1,
        frames
            .iter()
            .map(Vec::as_slice)
            .map(RawFrame::Rgb8)
            .collect(),
    );
    let out = encode(&clip, &EncodeOptions::default()).expect("encodes");
    assert!(is_webm(&out.data));
    assert_eq!(out.frame_count(), 2);
}

#[test]
fn rejects_invalid_parameters() {
    let frames = rgb_frames(WIDTH, HEIGHT, 2);
    let options = quality_options(120);
    let raw: Vec<RawFrame<'_>> = frames
        .iter()
        .map(Vec::as_slice)
        .map(RawFrame::Rgb8)
        .collect();

    let empty = Clip::new(WIDTH, HEIGHT, 30, 1, Vec::new());
    assert!(matches!(encode(&empty, &options), Err(Error::EmptyClip)));

    let zero = Clip::new(0, HEIGHT, 30, 1, raw.clone());
    assert!(matches!(
        encode(&zero, &options),
        Err(Error::InvalidDimensions { .. })
    ));

    let bad_fps = Clip::new(WIDTH, HEIGHT, 30, 0, raw.clone());
    assert!(matches!(
        encode(&bad_fps, &options),
        Err(Error::InvalidFps { .. })
    ));

    let bad_quality = EncodeOptions::quality(QUANTIZER_MAX + 1, SPEED_MAX);
    assert!(matches!(
        encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &bad_quality),
        Err(Error::InvalidQuantizer(_))
    ));

    let bad_speed = EncodeOptions::quality(120, SPEED_MAX + 1);
    assert!(matches!(
        encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &bad_speed),
        Err(Error::InvalidSpeed(_))
    ));

    let bad_bitrate = EncodeOptions::bitrate(0, SPEED_MAX);
    assert!(matches!(
        encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &bad_bitrate),
        Err(Error::InvalidBitrate(_))
    ));

    let short = vec![0u8; 10];
    let mismatch = Clip::new(WIDTH, HEIGHT, 30, 1, vec![RawFrame::Rgb8(short.as_slice())]);
    assert!(matches!(
        encode(&mismatch, &options),
        Err(Error::FrameSizeMismatch { .. })
    ));

    let bad_yuv = Clip::new(
        WIDTH,
        HEIGHT,
        30,
        1,
        vec![RawFrame::Yuv420 {
            y: short.as_slice(),
            u: short.as_slice(),
            v: short.as_slice(),
        }],
    );
    assert!(matches!(
        encode(&bad_yuv, &options),
        Err(Error::FrameSizeMismatch { .. })
    ));
}

#[test]
fn is_webm_and_probe_reject_other_bytes() {
    assert!(!is_webm(&[]));
    assert!(!is_webm(b"definitely not webm"));
    assert!(matches!(probe(&[]), Err(Error::InvalidContainer(_))));
    assert!(matches!(
        probe(b"definitely not webm"),
        Err(Error::InvalidContainer(_))
    ));
}

#[test]
fn probe_rejects_truncated_container() {
    let frames = rgb_frames(WIDTH, HEIGHT, FRAMES);
    let out = encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(120)).expect("encodes");
    assert!(out.data.len() > 64);
    // A prefix cut inside the EBML header cannot parse.
    assert!(matches!(
        probe(&out.data[..24]),
        Err(Error::InvalidContainer(_))
    ));
}

#[test]
fn probe_rejects_malformed_declared_segments_and_missing_track_fields() {
    fn element(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        master_element(&mut bytes, id, body);
        bytes
    }
    fn container(body: &[u8]) -> Vec<u8> {
        let mut bytes = element(&[0x1A, 0x45, 0xDF, 0xA3], &element(&[0x42, 0x82], b"webm"));
        bytes.extend(body);
        bytes
    }
    for bad_header in [vec![], vec![0], vec![0xEC, 0x84, 0], element(&[0xEC], &[])] {
        let bytes = element(&[0x1A, 0x45, 0xDF, 0xA3], &bad_header);
        assert!(!is_webm(&bytes));
        assert!(
            probe(&bytes)
                .unwrap_err()
                .to_string()
                .contains("not a WebM")
        );
    }
    assert!(
        probe(&container(&element(&[0xEC], &[])))
            .unwrap_err()
            .to_string()
            .contains("missing Segment")
    );
    let truncated = container(&[0x18, 0x53, 0x80, 0x67, 0x84, 0]);
    assert!(
        probe(&truncated)
            .unwrap_err()
            .to_string()
            .contains("truncated Segment")
    );
    let overrun = container(&element(&[0x18, 0x53, 0x80, 0x67], &[0xEC, 0x84, 0]));
    assert!(
        probe(&overrun)
            .unwrap_err()
            .to_string()
            .contains("overruns its parent")
    );
    for (codec, dimensions, expected) in [
        ("V_VP9", vec![], "no AV1 video track"),
        ("V_AV1", vec![], "misses dimensions"),
        (
            "V_AV1",
            vec![0xB0, 0x81, 32, 0xBA, 0x81, 32],
            "holds no frames",
        ),
    ] {
        let mut track = element(&[0x86], codec.as_bytes());
        track.extend(element(&[0xE0], &dimensions));
        let tracks = element(&[0x16, 0x54, 0xAE, 0x6B], &element(&[0xAE], &track));
        let segment = element(&[0x18, 0x53, 0x80, 0x67], &tracks);
        let bytes = container(&segment);
        assert!(is_webm(&bytes));
        let error = probe(&bytes).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn video_configuration_failures_keep_the_actual_encoder_cause() {
    let frames = rgb_frames(32, 32, 1);
    for (options, expected) in [
        (EncodeOptions::quality(QUANTIZER_MAX + 1, 10), "quantizer"),
        (EncodeOptions::quality(120, SPEED_MAX + 1), "speed"),
        (EncodeOptions::bitrate(0, 10), "bitrate"),
        (EncodeOptions::bitrate(u32::MAX, 10), "bitrate"),
    ] {
        let error = encode_rgb(&frames, 32, 32, 30, 1, &options).unwrap_err();
        assert!(error.to_string().contains(expected));
    }
    let empty = encode_rgb(&[], 32, 32, 30, 1, &quality_options(120)).unwrap_err();
    assert!(empty.to_string().contains("no frames"));
    let fps = encode_rgb(&frames, 32, 32, 0, 1, &quality_options(120)).unwrap_err();
    assert!(fps.to_string().contains("0/1"));
    // rav1e rejects video dimensions below 16; the facade preserves its cause.
    let small = encode_rgb(&[vec![0; 3 * 16]], 1, 16, 30, 1, &quality_options(120)).unwrap_err();
    assert!(matches!(small, Error::Encoder(_)));
    assert!(small.to_string().contains("InvalidWidth"));
}
