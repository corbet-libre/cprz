//! Real codec round trips and hostile bounded-decoding vectors on native/wasm.

use cprz_blob::portable::{self, Error};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn exact_bytes_survive_with_exact_limits() {
    for input in [
        Vec::new(),
        vec![0],
        (0..=255).collect(),
        b"private record\0\xff".repeat(2048),
    ] {
        let packet = portable::compress(&input).unwrap();
        assert_eq!(
            portable::decompress(&packet, packet.len(), input.len()).unwrap(),
            input
        );
        assert_eq!(portable::compress(&input).unwrap(), packet);
        assert!(matches!(
            portable::decompress(&packet, packet.len() - 1, input.len()),
            Err(Error::InputLimit)
        ));
        if !input.is_empty() {
            assert!(matches!(
                portable::decompress(&packet, packet.len(), input.len() - 1),
                Err(Error::OutputLimit)
            ));
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn oversized_expansion_never_returns_partial_plaintext() {
    let packet = portable::compress(&vec![0; 1024 * 1024]).unwrap();
    for limit in [0, 1, 4095, 4096, 4097, 65536] {
        assert!(matches!(
            portable::decompress(&packet, packet.len(), limit),
            Err(Error::OutputLimit)
        ));
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn truncated_unknown_and_concatenated_packets_fail_closed() {
    for packet in [&[][..], &[0], &[255, 1, 2]] {
        assert!(matches!(
            portable::decompress(packet, 100, 100),
            Err(Error::InvalidHeader)
        ));
    }
    let input = b"a bounded exact byte record".repeat(128);
    let packet = portable::compress(&input).unwrap();
    for end in 1..packet.len() {
        assert!(matches!(
            portable::decompress(&packet[..end], packet.len(), input.len()),
            Err(Error::InvalidStream)
        ));
    }
    for suffix in [vec![0], packet[1..].to_vec()] {
        let mut extra = packet.clone();
        extra.extend(suffix);
        assert!(matches!(
            portable::decompress(&extra, extra.len(), input.len()),
            Err(Error::TrailingBytes)
        ));
    }
    assert!(matches!(
        portable::decompress(&[portable::CODEC, 255, 255], 3, 100),
        Err(Error::InvalidStream)
    ));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn diagnostics_expose_no_payload() {
    use std::error::Error as _;
    for error in [
        Error::InvalidHeader,
        Error::InputLimit,
        Error::OutputLimit,
        Error::InvalidStream,
        Error::TrailingBytes,
    ] {
        assert!(!error.to_string().is_empty());
        assert!(error.source().is_none());
    }
    let error = Error::Encode(std::io::Error::other("codec failure"));
    assert_eq!(error.to_string(), "Brotli encoding failed");
    assert!(error.source().is_some());
}
