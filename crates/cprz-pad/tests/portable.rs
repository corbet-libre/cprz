//! Real Padme framing, including lengths that cannot fit wasm32 pointers.

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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn finite_size_classes_match_documented_framing() {
    // Framed length is data length plus the documented 8-byte prefix.
    assert_eq!(HEADER_LEN, 8);
    // Fixed mode: smallest power of two holding framed = len + 8.
    // Expected values are literals, including exact boundaries and the
    // first length over each boundary. All fit 32-bit and 64-bit pointers.
    let fixed_vectors = [
        (0_usize, 8_usize), // framed 8 -> 8
        (1, 16),            // framed 9 -> 16
        (8, 16),            // framed 16 -> 16
        (9, 32),            // framed 17 -> 32
        (24, 32),           // framed 32 -> 32
        (25, 64),           // framed 33 -> 64
        (100, 128),         // framed 108 -> 128
        (248, 256),         // framed 256 -> 256
        (249, 512),         // framed 257 -> 512
    ];
    for (len, expected) in fixed_vectors {
        assert_eq!(fixed::padded_len(len), expected, "fixed len {len}");
    }
    // Default Padme mode: reference vectors checked against PURBs Section 4.
    // Framed = len + 8; expected is the class representative as a literal.
    let padme_vectors = [
        (0_usize, 8_usize), // framed 8 -> 8
        (1, 10),            // framed 9 -> 10
        (8, 16),            // framed 16 -> 16
        (16, 24),           // framed 24 -> 24
        (1000, 1024),       // framed 1008 -> 1024
        (100_000, 100_352), // framed 100008 -> 100352
    ];
    for (len, expected) in padme_vectors {
        assert_eq!(padded_len(len), expected, "padme len {len}");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn pointer_width_boundaries_saturate_without_allocating() {
    // Near-max inputs cannot be framed once HEADER_LEN is added, so both
    // oracles report the documented saturation instead of panicking or
    // wrapping. No buffers are allocated here; only lengths are computed.
    for len in [
        usize::MAX,
        usize::MAX - 1,
        usize::MAX - HEADER_LEN,
        usize::MAX - HEADER_LEN + 1,
    ] {
        assert_eq!(padded_len(len), usize::MAX, "padme saturation");
        assert_eq!(fixed::padded_len(len), usize::MAX, "fixed saturation");
    }
    // A declared length equal to the pointer maximum is refused from the
    // 8-byte prefix alone, without allocating the declared size.
    let declared = usize::MAX as u64;
    assert!(
        matches!(unpad(&declared.to_be_bytes()), Err(Error::InvalidLength { declared: actual, available: 0 }) if actual == declared)
    );
}
