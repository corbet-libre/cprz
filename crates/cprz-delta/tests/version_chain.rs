//! Sequential binary version chain over the public delta API.
//!
//! Builds a deterministic series of 4-16 KiB binary versions containing
//! `0x00` and `0xFF`, diffs each version against the previously
//! reconstructed bytes with [`cprz_delta::diff`], rebuilds it with
//! [`cprz_delta::apply`], and checks the exact expected bytes at every step.

use cprz_delta::{MODE_DELTA, MODE_FULL, apply, diff};

/// Deterministic binary payload covering every byte value, including
/// `0x00` and `0xFF`.
fn patterned(len: usize) -> Vec<u8> {
    (0..=255u8).cycle().take(len).collect()
}

fn assert_generic_binary(version: &[u8]) {
    assert!(
        (4096..=16 * 1024).contains(&version.len()),
        "version is {} bytes, expected 4-16 KiB",
        version.len()
    );
    assert!(version.contains(&0), "version must contain 0x00");
    assert!(version.contains(&255), "version must contain 0xFF");
}

/// Insertion block with the same deterministic byte coverage.
fn insertion_block(len: usize) -> Vec<u8> {
    (0..=255u8).rev().cycle().take(len).collect()
}

#[test]
fn binary_version_chain_rebuilds_each_step_from_reconstructed_base() {
    // First version: 8 KiB of generic deterministic binary bytes.
    let first = patterned(8 * 1024);

    // Second version: same length with changed bytes only.
    let mut second = first.clone();
    second[0] = 255;
    second[1] = 0;
    second[4096] ^= 0xFF;
    second[8191] ^= 0x01;
    second[2000..2010].copy_from_slice(&[0xFF; 10]);

    // Third version: insertion of 512 deterministic bytes.
    let mut third = second.clone();
    let insert = insertion_block(512);
    third.splice(3000..3000, insert.iter().copied());

    // Fourth version: deletion of 1024 bytes.
    let mut fourth = third.clone();
    fourth.drain(1000..2024);

    // Fifth version: changed bytes plus a small deletion and insertion.
    let mut fifth = fourth.clone();
    fifth[500] ^= 0xA5;
    fifth[1500..1516].copy_from_slice(&[
        0x00, 0xFF, 0x00, 0xFF, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    ]);
    fifth.drain(6000..6128);
    let tail_insert = insertion_block(256);
    fifth.splice(7000..7000, tail_insert.iter().copied());

    let versions = [first, second, third, fourth, fifth];
    for version in &versions {
        assert_generic_binary(version);
    }

    // Replay the chain: each delta is built against the previously
    // reconstructed bytes, never against the pristine fixture copy.
    let mut reconstructed: Vec<u8> = Vec::new();
    for (step, expected) in versions.iter().enumerate() {
        let delta = diff(&reconstructed, expected);
        assert!(!delta.is_empty(), "step {step}: delta packet is empty");
        let mode = delta[0];
        assert!(
            mode == MODE_DELTA || mode == MODE_FULL,
            "step {step}: unexpected mode {mode}"
        );
        if step == 0 {
            // An empty base cannot serve as a dictionary, so the first
            // version is always stored standalone.
            assert_eq!(mode, MODE_FULL, "step 0: empty base must use full mode");
        }
        reconstructed = apply(&reconstructed, &delta)
            .unwrap_or_else(|error| panic!("step {step}: apply failed: {error}"));
        assert_eq!(
            &reconstructed, expected,
            "step {step}: reconstructed bytes differ from expected version"
        );
    }
}
