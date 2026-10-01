use super::{fixed, pad, padded_len, unpad};

/// Deterministic pseudo-random filler so tests never rely on zero data.
fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i.wrapping_mul(31).wrapping_add(len) & 0xff) as u8)
        .collect()
}

/// Lengths 0..=1024 exhaustive, then a stride up to 100000.
fn sample_lengths() -> Vec<usize> {
    let mut out: Vec<usize> = (0..=1024).collect();
    let mut n = 1025;
    while n <= 100_000 {
        out.push(n);
        n += 13;
    }
    out.push(99_999);
    out.push(100_000);
    out
}

#[test]
fn oracle_matches_reference_vectors() {
    // Checked against the PURBs Section 4 construction: each expected
    // value is the Padmé oracle applied to framed = data_len + 8.
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
        (100_000, 100_352),
    ];
    for (len, expected) in vectors {
        assert_eq!(padded_len(len), expected, "len {len}");
    }
}

#[test]
fn oracle_overhead_stays_within_bound() {
    // Pure oracle (without the framing header) over every length:
    // output covers the input and overhead stays at about 12%.
    for len in 1..=100_000_usize {
        let out = super::padme_padded_len(len as u64) as usize;
        assert!(out >= len, "len {len}");
        assert!(
            (out - len) * 100 <= len * 12,
            "len {len}: {out} exceeds 12% overhead"
        );
    }
    assert_eq!(super::padme_padded_len(0), 0);
}

#[test]
fn padded_output_is_a_class_member() {
    // Padding an already padded length changes nothing: outputs are
    // fixed points of the oracle (idempotent size classes).
    for len in sample_lengths() {
        let out = padded_len(len) as u64;
        assert_eq!(super::padme_padded_len(out), out, "len {len}");
    }
}

#[test]
fn roundtrip_sampled_lengths() {
    for len in sample_lengths() {
        let data = pattern(len);
        let padded = pad(&data);
        assert_eq!(padded.len(), padded_len(len), "len {len}");
        assert_eq!(unpad(&padded), Ok(data), "len {len}");
    }
}

#[test]
fn framed_overhead_stays_within_bound() {
    // With the 8-byte header amortised (len >= 256) the total overhead
    // of `pad` stays within about 12% up to 100000.
    for len in sample_lengths().into_iter().filter(|&n| n >= 256) {
        let out = padded_len(len);
        assert!(out >= len + super::HEADER_LEN, "len {len}");
        assert!(
            (out - len) * 100 <= len * 12,
            "len {len}: {out} exceeds 12% overhead"
        );
    }
}

#[test]
fn unpad_rejects_malformed_input() {
    assert_eq!(unpad(&[]), Err(super::Error::Empty));
    assert_eq!(unpad(&[0u8; 7]), Err(super::Error::Empty));

    // Declared length larger than the buffer holds.
    let mut oversized = [0u8; 8];
    oversized.copy_from_slice(&9u64.to_be_bytes());
    assert_eq!(
        unpad(&oversized),
        Err(super::Error::InvalidLength {
            declared: 9,
            available: 0,
        })
    );

    // Truncated into the data area.
    let padded = pad(b"hello");
    assert_eq!(
        unpad(&padded[..10]),
        Err(super::Error::InvalidLength {
            declared: 5,
            available: 2,
        })
    );

    // Corrupted padding byte.
    let mut tampered = pad(b"hello");
    let last = tampered.len() - 1;
    tampered[last] = 0xff;
    assert_eq!(unpad(&tampered), Err(super::Error::NonZeroPadding));
}

#[test]
fn empty_input_roundtrips() {
    let padded = pad(&[]);
    assert_eq!(padded.len(), padded_len(0));
    assert_eq!(unpad(&padded), Ok(Vec::new()));
}

#[test]
fn fixed_classes_roundtrip() {
    let vectors = [(0_usize, 8_usize), (1, 16), (8, 16), (9, 32), (100, 128)];
    for (len, expected) in vectors {
        assert_eq!(fixed::padded_len(len), expected, "len {len}");
    }
    for len in sample_lengths() {
        let data = pattern(len);
        let padded = fixed::pad(&data);
        let framed = len + super::HEADER_LEN;
        assert_eq!(padded.len(), fixed::padded_len(len), "len {len}");
        assert!(padded.len().is_power_of_two(), "len {len}");
        assert!(padded.len() >= framed, "len {len}");
        assert!(padded.len() < 2 * framed, "len {len}");
        assert_eq!(unpad(&padded), Ok(data.clone()), "len {len}");
        assert_eq!(fixed::unpad(&padded), Ok(data), "len {len}");
    }
}

#[test]
fn huge_lengths_saturate_instead_of_wrapping() {
    assert_eq!(padded_len(usize::MAX), usize::MAX);
    assert_eq!(fixed::padded_len(usize::MAX), usize::MAX);
}
