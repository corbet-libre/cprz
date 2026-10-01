use super::*;

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
    let out =
        encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(120)).expect("encodes");
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
    let high =
        encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(40)).expect("encodes");
    let low =
        encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(220)).expect("encodes");
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
    let fast =
        encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(120)).expect("encodes");
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
    let out =
        encode_rgb(&frames, WIDTH, HEIGHT, 30, 1, &quality_options(120)).expect("encodes");
    assert!(out.data.len() > 64);
    // A prefix cut inside the EBML header cannot parse.
    assert!(matches!(
        probe(&out.data[..24]),
        Err(Error::InvalidContainer(_))
    ));
}
