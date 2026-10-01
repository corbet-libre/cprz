use super::*;

/// Deterministic pseudo-random bytes from a splitmix64 stream, so tests
/// need no extra dependencies yet still exercise incompressible data.
fn pseudo_random(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        state = state
            .wrapping_add(0x9E37_79B9_7F4A_7C15)
            .wrapping_mul(0xBF58_476D_1CE4_E5B9);
        state ^= state >> 29;
        state = state.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        state ^= state >> 32;
        out.extend_from_slice(&state.to_le_bytes());
    }
    out.truncate(len);
    out
}

/// A long, repetitive text payload: small edits must delta to almost
/// nothing against it.
fn large_text() -> Vec<u8> {
    let paragraph = "The quick brown fox jumps over the lazy dog. \
        Pack my box with five dozen liquor jugs. \
        How vexingly quick daft zebras jump! ";
    paragraph.repeat(2000).into_bytes()
}

#[test]
fn small_edits_in_large_text_produce_tiny_deltas() {
    let old = large_text();
    let mut new = old.clone();
    new.splice(10_000..10_011, b"swift red wolf".iter().copied());
    new.splice(80_000..80_005, b"owls!".iter().copied());
    new.extend_from_slice(b"One more line at the end.");

    let delta = diff(&old, &new);
    assert_eq!(delta[0], MODE_DELTA);
    assert!(
        delta.len() * 50 < new.len(),
        "delta {} bytes is not tiny against {} bytes",
        delta.len(),
        new.len()
    );
    assert_eq!(apply(&old, &delta).unwrap(), new);
}

#[test]
fn identical_versions_produce_tiny_deltas() {
    let old = large_text();
    let delta = diff(&old, &old);
    assert_eq!(delta[0], MODE_DELTA);
    assert!(delta.len() * 50 < old.len());
    assert_eq!(apply(&old, &delta).unwrap(), old);
}

#[test]
fn unrelated_data_falls_back_to_full() {
    let old = pseudo_random(20_000, 0x1234_5678);
    let new = pseudo_random(20_000, 0xDEAD_BEEF);
    let delta = diff(&old, &new);
    assert_eq!(delta[0], MODE_FULL);
    assert_eq!(apply(&old, &delta).unwrap(), new);
    // Applying a full-mode delta ignores `old` entirely.
    assert_eq!(apply(&pseudo_random(20_000, 1), &delta).unwrap(), new);
}

#[test]
fn binary_round_trips_hold() {
    let mut old: Vec<u8> = (0..=255u8).cycle().take(30_000).collect();
    let new = old.clone();
    assert_eq!(apply(&old, &diff(&old, &new)).unwrap(), new);

    old[29_999] ^= 0xFF;
    assert_eq!(apply(&old, &diff(&old, &old)).unwrap(), old);
}

#[test]
fn empty_inputs_round_trip() {
    assert_eq!(apply(b"", &diff(b"", b"")).unwrap(), b"");
    assert_eq!(apply(b"old", &diff(b"old", b"")).unwrap(), b"");
    let delta = diff(b"", b"brand new");
    assert_eq!(delta[0], MODE_FULL);
    assert_eq!(apply(b"", &delta).unwrap(), b"brand new");
    assert_eq!(apply(b"something else", &delta).unwrap(), b"brand new");
}

#[test]
fn rejects_malformed_deltas() {
    assert!(matches!(apply(b"old", b""), Err(Error::Empty)));
    assert!(matches!(
        apply(b"old", &[0x09]),
        Err(Error::UnknownMode(0x09))
    ));

    // Truncated frame: header names full mode, payload is cut short.
    let mut delta = diff(b"old", b"some new payload");
    assert_eq!(delta[0], MODE_FULL);
    delta.truncate(delta.len() / 2);
    assert!(matches!(apply(b"old", &delta), Err(Error::Decode(_))));

    // Delta applied against an unrelated base version fails: the bytes the
    // delta references come out different, so the checksum does not match.
    let old = large_text();
    let mut new = old.clone();
    new.splice(1000..1010, b"xxxxxxxxxx".iter().copied());
    let delta = diff(&old, &new);
    assert_eq!(delta[0], MODE_DELTA);
    let wrong_old = pseudo_random(old.len(), 0x5EED);
    assert!(matches!(apply(&wrong_old, &delta), Err(Error::Decode(_))));
}
