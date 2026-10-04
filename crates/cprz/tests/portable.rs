//! Public facade uses the same bounded portable codec on native and wasm.
//!
//! Composition vectors prove the documented order compress -> pad -> encrypt:
//! these exact bytes survive the first two stages, and hostile input fails
//! closed at the stage that owns it. Padding is not encryption.

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn facade_preserves_exact_bytes() {
    let input = b"profile bytes\0\xff";
    let packet = cprz::portable::compress(input).unwrap();
    let decoded = cprz::portable::decompress(&packet, packet.len(), input.len()).unwrap();
    assert_eq!(decoded, input);
}

/// Compress -> pad -> unpad -> decompress round trips and failure vectors.
///
/// `cprz-pad` is only linked with the `native` feature, so the portable
/// no-default-features run keeps the exact-byte facade test above while this
/// module compiles out.
#[cfg(feature = "native")]
mod composition {
    use cprz::portable::{self, Error};
    use cprz_pad::{pad, padded_len, unpad};

    /// Deterministic binary filler with embedded zeros and non-UTF8 bytes.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| (i.wrapping_mul(31).wrapping_add(len) & 0xff) as u8)
            .collect()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn compress_pad_unpad_decompress_preserves_exact_bytes() {
        // Data lengths at existing Padme oracle vectors, plus empty input and
        // explicit all-zero, all-0xff, embedded-zero and full-range fixtures.
        let mut inputs: Vec<Vec<u8>> = [0_usize, 1, 2, 3, 8, 9, 15, 16, 17, 25, 33, 92, 121]
            .into_iter()
            .map(pattern)
            .collect();
        inputs.push(vec![0; 64]);
        inputs.push(vec![0xff; 64]);
        inputs.push(b"profile bytes\0\xff".to_vec());
        inputs.push((0..=255u8).collect());
        for input in inputs {
            let packet = portable::compress(&input).unwrap();
            let padded = pad(&packet);
            assert_eq!(padded.len(), padded_len(packet.len()));
            let framed = unpad(&padded).unwrap();
            assert_eq!(framed, packet);
            assert_eq!(
                portable::decompress(&framed, framed.len(), input.len()).unwrap(),
                input
            );
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn composition_enforces_exact_encoded_and_decoded_limits() {
        let input = pattern(321);
        let packet = portable::compress(&input).unwrap();
        let framed = unpad(&pad(&packet)).unwrap();
        assert_eq!(
            portable::decompress(&framed, framed.len(), input.len()).unwrap(),
            input
        );
        assert!(matches!(
            portable::decompress(&framed, framed.len() - 1, input.len()),
            Err(Error::InputLimit)
        ));
        assert!(matches!(
            portable::decompress(&framed, framed.len(), input.len() - 1),
            Err(Error::OutputLimit)
        ));
        let empty_packet = portable::compress(&[]).unwrap();
        let empty_framed = unpad(&pad(&empty_packet)).unwrap();
        assert_eq!(
            portable::decompress(&empty_framed, empty_framed.len(), 0).unwrap(),
            Vec::<u8>::new()
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn composition_refuses_hostile_vectors_at_the_owning_stage() {
        let input = pattern(200);
        let packet = portable::compress(&input).unwrap();
        assert_eq!(packet[0], portable::CODEC);

        // Unknown codec byte survives unpadding, then fails at decompress.
        let mut unknown = packet.clone();
        unknown[0] = portable::CODEC.wrapping_add(1);
        let framed = unpad(&pad(&unknown)).unwrap();
        assert_eq!(framed, unknown);
        assert!(matches!(
            portable::decompress(&framed, framed.len(), input.len()),
            Err(Error::InvalidHeader)
        ));

        // Truncated packet fails at decompress.
        let truncated = &packet[..packet.len() - 1];
        assert!(matches!(
            portable::decompress(truncated, truncated.len(), input.len()),
            Err(Error::InvalidStream)
        ));

        // Buffer cut into the framed packet fails at unpad with exact counts.
        let padded = pad(&packet);
        let cut = &padded[..cprz_pad::HEADER_LEN + packet.len() - 1];
        assert_eq!(
            unpad(cut),
            Err(cprz_pad::Error::InvalidLength {
                declared: packet.len() as u64,
                available: packet.len() - 1,
            })
        );

        // The extra byte is inside the declared payload after pad(trailing),
        // not padding: unpad preserves it and decompress refuses the trailing
        // stream bytes.
        let mut trailing = packet.clone();
        trailing.push(0);
        let framed = unpad(&pad(&trailing)).unwrap();
        assert_eq!(framed, trailing);
        assert!(matches!(
            portable::decompress(&framed, framed.len(), input.len()),
            Err(Error::TrailingBytes)
        ));

        // A trailing non-zero byte is not valid padding.
        let mut tampered = pad(&packet);
        tampered.push(0xff);
        assert_eq!(unpad(&tampered), Err(cprz_pad::Error::NonZeroPadding));
    }
}
