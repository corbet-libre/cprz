//! Unicode byte-preservation vectors for `cprz-text`.
//!
//! Every vector round-trips through the public `compress`/`decompress` API
//! with no normalization: byte-distinct inputs must decode to byte-distinct
//! outputs. Only the composed/decomposed accent pair is canonically
//! equivalent Unicode; the BOM and line-ending pairs are simply different
//! strings that must not be merged by stripping or line-ending conversion.
//! CJK/emoji, embedded NUL, and mixed RTL text must decode to their exact
//! input bytes.

use cprz_text::{compress, decompress};

/// Composed form: `caf` + U+00E9.
const COMPOSED: &str = "caf\u{E9} order 42";
/// Decomposed form: `cafe` + combining acute U+0301.
const DECOMPOSED: &str = "cafe\u{301} order 42";
/// Leading BOM kept as the actual character U+FEFF.
const WITH_BOM: &str = "\u{FEFF}hello world 42";
/// Same text without the BOM.
const WITHOUT_BOM: &str = "hello world 42";
/// Windows line endings.
const WITH_CRLF: &str = "line one\r\nline two\r\nend 42";
/// Unix line endings.
const WITH_LF: &str = "line one\nline two\nend 42";
/// CJK text mixed with emoji.
const CJK_EMOJI: &str = "\u{65E5}\u{672C}\u{8A9E}\u{30C6}\u{30B9}\u{30C8}\u{1F389}\u{1F30D} 42";
/// Embedded NUL bytes mixed with Latin and RTL scripts (Hebrew, Arabic).
const NUL_RTL_MIXED: &str = "abc\u{0}def \u{5E9}\u{5DC}\u{5D5}\u{5DD} 42 \u{627}\u{644}\u{639}\u{631}\u{628}\u{64A}\u{629}\u{0}end";
/// Small generic public dictionary (pangram plus digits), never member data.
const GENERIC_DICT: &[u8] = b"the quick brown fox jumps over the lazy dog 0123456789 ";

fn roundtrip(text: &str) -> String {
    let packet = compress(text, None).expect("compress must succeed");
    decompress(&packet, None).expect("decompress must succeed")
}

fn roundtrip_with_dict(text: &str, dictionary: &[u8]) -> String {
    let packet = compress(text, Some(dictionary)).expect("compress must succeed");
    decompress(&packet, Some(dictionary)).expect("decompress must succeed")
}

#[test]
fn composed_and_decomposed_accents_stay_byte_distinct() {
    assert_ne!(
        COMPOSED.as_bytes(),
        DECOMPOSED.as_bytes(),
        "fixtures must start byte-distinct"
    );
    let composed_back = roundtrip(COMPOSED);
    let decomposed_back = roundtrip(DECOMPOSED);
    assert_eq!(composed_back.as_bytes(), COMPOSED.as_bytes());
    assert_eq!(decomposed_back.as_bytes(), DECOMPOSED.as_bytes());
    assert_ne!(
        composed_back.as_bytes(),
        decomposed_back.as_bytes(),
        "normalization must not merge the pair"
    );
}

#[test]
fn bom_is_preserved_as_actual_u_feff() {
    assert_ne!(
        WITH_BOM.as_bytes(),
        WITHOUT_BOM.as_bytes(),
        "fixtures must start byte-distinct"
    );
    let bom_back = roundtrip(WITH_BOM);
    let plain_back = roundtrip(WITHOUT_BOM);
    assert_eq!(bom_back.as_bytes(), WITH_BOM.as_bytes());
    assert_eq!(plain_back.as_bytes(), WITHOUT_BOM.as_bytes());
    assert!(
        bom_back.starts_with('\u{FEFF}'),
        "leading U+FEFF must survive the round trip"
    );
    assert_ne!(
        bom_back.as_bytes(),
        plain_back.as_bytes(),
        "BOM stripping must not merge the pair"
    );
}

#[test]
fn crlf_and_lf_stay_byte_distinct() {
    assert_ne!(
        WITH_CRLF.as_bytes(),
        WITH_LF.as_bytes(),
        "fixtures must start byte-distinct"
    );
    let crlf_back = roundtrip(WITH_CRLF);
    let lf_back = roundtrip(WITH_LF);
    assert_eq!(crlf_back.as_bytes(), WITH_CRLF.as_bytes());
    assert_eq!(lf_back.as_bytes(), WITH_LF.as_bytes());
    assert_ne!(
        crlf_back.as_bytes(),
        lf_back.as_bytes(),
        "line-ending normalization must not merge the pair"
    );
}

#[test]
fn cjk_emoji_and_nul_rtl_mixed_round_trip_exact() {
    let cjk_back = roundtrip(CJK_EMOJI);
    assert_eq!(cjk_back.as_bytes(), CJK_EMOJI.as_bytes());
    let mixed_back = roundtrip(NUL_RTL_MIXED);
    assert_eq!(mixed_back.as_bytes(), NUL_RTL_MIXED.as_bytes());
    assert!(
        mixed_back.as_bytes().contains(&0),
        "embedded NUL bytes must survive"
    );
}

#[test]
fn generic_dictionary_variant_round_trips_exact() {
    let back = roundtrip_with_dict(WITH_LF, GENERIC_DICT);
    assert_eq!(back.as_bytes(), WITH_LF.as_bytes());
}
