use super::*;

#[test]
fn degenerate_dimensions_and_extreme_budgets_preserve_resize_contracts() {
    for dimensions in [(0, 10, 10), (10, 0, 10), (10, 10, 0)] {
        assert_eq!(
            resized_dimensions(dimensions.0, dimensions.1, dimensions.2),
            (0, 0)
        );
    }
    assert!(!is_avif(b"\0\0\0\x0cftypjpeg"));
    assert!(!is_avif(b"\0\0\0\x0cotheravif"));
    let input = encode_input(&gradient(8, 8), ImageFormat::Png);
    let generous = encode_avif_to_size(&input, 8, 1_000_000, 10).unwrap();
    assert!(is_avif(&generous.data));
    let impossible = encode_avif_to_size(&input, 8, 1, 10).unwrap();
    assert!(impossible.data.len() > 1);
    assert_eq!(
        (generous.width, generous.height),
        (impossible.width, impossible.height)
    );
}
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use std::io::Cursor;

/// Deterministic gradient with enough detail to compress non-trivially.
fn gradient(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        let r = ((x * 255 / width.max(1)) ^ (y * 255 / height.max(1))) as u8;
        let g = ((x * 7 + y * 13) % 256) as u8;
        let b = ((x * x + y * y) % 256) as u8;
        Rgb([r, g, b])
    }))
}

fn encode_input(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), format)
        .expect("test image encodes");
    bytes
}

#[test]
fn valid_avif_from_png() {
    let input = encode_input(&gradient(64, 48), ImageFormat::Png);
    let out = encode_avif(&input, 64, 60.0, 10).expect("encodes");
    assert!(is_avif(&out.data));
    assert_eq!(out.dimensions(), (64, 48));
    assert_eq!(out.byte_size(), out.data.len());
    let info = output_info(&out);
    assert_eq!(
        (info.width, info.height, info.byte_size),
        (64, 48, out.data.len())
    );
    assert_eq!(out.info(), info);
}

#[test]
fn resize_keeps_aspect_ratio() {
    let input = encode_input(&gradient(160, 90), ImageFormat::Png);
    let out = encode_avif(&input, 64, 60.0, 10).expect("encodes");
    assert_eq!(out.dimensions(), (64, 36));
    assert!(out.width <= 64 && out.height <= 64);
}

#[test]
fn small_images_are_not_upscaled() {
    let input = encode_input(&gradient(16, 12), ImageFormat::Png);
    let out = encode_avif(&input, 128, 60.0, 10).expect("encodes");
    assert_eq!(out.dimensions(), (16, 12));
}

#[test]
fn jpeg_input_decodes() {
    let input = encode_input(&gradient(48, 32), ImageFormat::Jpeg);
    let out = encode_avif(&input, 48, 60.0, 10).expect("encodes");
    assert!(is_avif(&out.data));
    assert_eq!(out.dimensions(), (48, 32));
}

#[test]
fn webp_input_decodes() {
    let input = encode_input(&gradient(48, 32), ImageFormat::WebP);
    let out = encode_avif(&input, 48, 60.0, 10).expect("encodes");
    assert!(is_avif(&out.data));
    assert_eq!(out.dimensions(), (48, 32));
}

#[test]
fn target_size_is_respected() {
    let input = encode_input(&gradient(96, 96), ImageFormat::Png);
    let target = 2500;
    let out = encode_avif_to_size(&input, 96, target, 10).expect("encodes");
    assert!(is_avif(&out.data));
    assert!(
        out.byte_size() <= target,
        "output is {} bytes, target {target}",
        out.byte_size()
    );
}

#[test]
fn higher_quality_is_not_smaller() {
    let input = encode_input(&gradient(64, 64), ImageFormat::Png);
    let low = encode_avif(&input, 64, 30.0, 10).expect("encodes");
    let high = encode_avif(&input, 64, 90.0, 10).expect("encodes");
    assert!(high.byte_size() >= low.byte_size());
}

#[test]
fn rejects_invalid_parameters() {
    let input = encode_input(&gradient(16, 16), ImageFormat::Png);
    assert!(matches!(
        encode_avif(&[], 64, 60.0, 10),
        Err(Error::Decode(_))
    ));
    assert!(matches!(
        encode_avif(&input, 0, 60.0, 10),
        Err(Error::InvalidMaxDimension)
    ));
    assert!(matches!(
        encode_avif(&input, 64, 0.0, 10),
        Err(Error::InvalidQuality(_))
    ));
    assert!(matches!(
        encode_avif(&input, 64, 101.0, 10),
        Err(Error::InvalidQuality(_))
    ));
    assert!(matches!(
        encode_avif(&input, 64, f32::NAN, 10),
        Err(Error::InvalidQuality(_))
    ));
    assert!(matches!(
        encode_avif(&input, 64, 60.0, 0),
        Err(Error::InvalidSpeed(_))
    ));
    assert!(matches!(
        encode_avif(&input, 64, 60.0, 11),
        Err(Error::InvalidSpeed(_))
    ));
    assert!(matches!(
        encode_avif_to_size(&input, 64, 0, 10),
        Err(Error::InvalidTargetSize)
    ));
}

#[test]
fn is_avif_rejects_other_bytes() {
    let png = encode_input(&gradient(8, 8), ImageFormat::Png);
    assert!(!is_avif(&png));
    assert!(!is_avif(&[]));
    assert!(!is_avif(b"ftyp"));
}

#[test]
fn image_failure_diagnostics_retain_decode_causes_and_invalid_parameters() {
    use std::error::Error as _;
    let input = encode_input(&gradient(8, 8), ImageFormat::Png);
    for (error, expected, nested) in [
        (
            encode_avif(b"not an image", 8, 50.0, 10).unwrap_err(),
            "decode",
            true,
        ),
        (
            encode_avif(&input, 8, 0.0, 10).unwrap_err(),
            "quality",
            false,
        ),
        (encode_avif(&input, 8, 50.0, 0).unwrap_err(), "speed", false),
        (
            encode_avif_to_size(&input, 8, 0, 10).unwrap_err(),
            "target byte size",
            false,
        ),
    ] {
        assert!(error.to_string().contains(expected));
        assert_eq!(error.source().is_some(), nested);
    }
}

#[test]
fn a_real_upstream_avif_refusal_preserves_its_diagnostic_through_conversion() {
    use std::error::Error as _;
    // ravif's raw-plane API rejects a short iterator before encoding a frame.
    let upstream = match ravif::Encoder::new()
        .with_speed(10)
        .encode_raw_planes_8_bit(
            8,
            8,
            std::iter::empty::<[u8; 3]>(),
            None::<Vec<u8>>,
            ravif::PixelRange::Full,
            ravif::MatrixCoefficients::Identity,
        ) {
        Err(error) => error,
        Ok(_) => panic!("short raw frame must fail"),
    };
    assert!(matches!(upstream, ravif::Error::TooFewPixels));
    let original = upstream.to_string();
    let error = Error::from(upstream);
    assert!(matches!(error, Error::Encode(_)));
    assert!(error.to_string().contains(&original));
    assert!(error.source().is_none());
}
