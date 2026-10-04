//! Real Padme framing, including lengths that cannot fit wasm32 pointers.
//!
//! Boundary vectors pin the existing Padme size classes for binary payloads
//! with embedded zeros and non-UTF8 bytes; hostile buffers fail closed with
//! exact counts and without allocating declared sizes.

use cprz_pad::{Error, HEADER_LEN, fixed, pad, padded_len, unpad};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn exact_framing_and_oversized_prefixes_refuse_without_allocating_declared_sizes() {
    for bytes in [vec![], vec![0, 255], (0..=255).collect()] {
        assert_eq!(unpad(&pad(&bytes)).unwrap(), bytes);
        assert_eq!(unpad(&fixed::pad(&bytes)).unwrap(), bytes);
    }
    for declared in [u64::from(u32::MAX) + 1, u64::MAX] {
        assert!(
            matches!(unpad(&declared.to_be_bytes()), Err(Error::InvalidLength { declared: actual, available: 0 }) if actual == declared)
        );
    }
}

/// Deterministic binary filler with embedded zeros and non-UTF8 bytes.
fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i.wrapping_mul(31).wrapping_add(len) & 0xff) as u8)
        .collect()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn binary_payloads_roundtrip_at_existing_padme_boundaries() {
    // Expected padded lengths are the existing Padme oracle vectors: each is
    // the oracle applied to framed = data_len + HEADER_LEN.
    let vectors = [
        (0_usize, 8_usize),
        (1, 10),
        (2, 10),
        (3, 12),
        (8, 16),
        (9, 18),
        (15, 24),
        (16, 24),
        (17, 26),
        (25, 36),
        (33, 44),
        (92, 104),
        (121, 144),
        (1000, 1024),
    ];
    for (len, expected) in vectors {
        for data in [vec![0u8; len], vec![0xffu8; len], pattern(len)] {
            let padded = pad(&data);
            assert_eq!(padded.len(), expected, "len {len}");
            assert_eq!(padded.len(), padded_len(len), "len {len}");
            assert_eq!(
                &padded[..HEADER_LEN],
                &(len as u64).to_be_bytes(),
                "len {len}"
            );
            assert_eq!(unpad(&padded).unwrap(), data, "len {len}");
        }
    }
    // Full byte range and embedded zero / non-UTF8 fixtures.
    let full_range: Vec<u8> = (0..=255).collect();
    assert_eq!(unpad(&pad(&full_range)).unwrap(), full_range);
    for fixture in [
        b"\0".as_slice(),
        b"\xff".as_slice(),
        b"record\0bytes\xff\xfe".as_slice(),
    ] {
        assert_eq!(unpad(&pad(fixture)).unwrap(), fixture);
    }
    // Fixed power-of-two classes round-trip the same binary shapes.
    for (len, expected) in [(0_usize, 8_usize), (1, 16), (8, 16), (9, 32), (100, 128)] {
        assert_eq!(fixed::padded_len(len), expected, "len {len}");
        for data in [vec![0u8; len], vec![0xffu8; len], pattern(len)] {
            let padded = fixed::pad(&data);
            assert_eq!(padded.len(), expected, "len {len}");
            assert!(padded.len().is_power_of_two(), "len {len}");
            assert_eq!(unpad(&padded).unwrap(), data, "len {len}");
            assert_eq!(fixed::unpad(&padded).unwrap(), data, "len {len}");
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn truncated_and_tampered_buffers_refuse_with_exact_counts() {
    assert_eq!(unpad(&[]), Err(Error::Empty));
    assert_eq!(unpad(&[0u8; HEADER_LEN - 1]), Err(Error::Empty));

    // Buffer cut one byte into the framed data reports exact counts.
    let padded = pad(&[0xabu8; 33]);
    let cut = &padded[..HEADER_LEN + 32];
    assert_eq!(
        unpad(cut),
        Err(Error::InvalidLength {
            declared: 33,
            available: 32,
        })
    );

    // Declared length larger than the buffer holds.
    let mut oversized = [0u8; HEADER_LEN];
    oversized.copy_from_slice(&9u64.to_be_bytes());
    assert_eq!(
        unpad(&oversized),
        Err(Error::InvalidLength {
            declared: 9,
            available: 0,
        })
    );

    // An appended non-zero byte is not valid padding.
    let mut tampered = pad(b"binary\0\xff");
    tampered.push(0xff);
    assert_eq!(unpad(&tampered), Err(Error::NonZeroPadding));
}
