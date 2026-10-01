//! Real Padme framing, including lengths that cannot fit wasm32 pointers.

use cprz_pad::{Error, fixed, pad, unpad};

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
