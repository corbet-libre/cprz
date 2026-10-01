//! Public facade uses the same bounded portable codec on native and wasm.

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn facade_preserves_exact_bytes() {
    let input = b"profile bytes\0\xff";
    let packet = cprz::portable::compress(input).unwrap();
    let decoded = cprz::portable::decompress(&packet, packet.len(), input.len()).unwrap();
    assert_eq!(decoded, input);
}
