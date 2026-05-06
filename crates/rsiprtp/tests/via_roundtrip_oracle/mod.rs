//! Per-header round-trip oracle for `Via`: parse → serialize →
//! reparse → serialize → reparse must be a fixed point at the
//! second parse.
//!
//! See `wrk_docs/2026.05.06 - HLD - per-header roundtrip fuzz oracle.md`
//! for the full design. Counterpart to the whole-message oracle at
//! `tests/parser_roundtrip_oracle/mod.rs`, scoped to a single typed
//! header so a libfuzzer corpus mutator can hunt for serializer
//! asymmetries on bytes-level inputs.
//!
//! # Triage note
//!
//! A panic from this oracle is a **parser/serializer asymmetry**
//! finding, not a parser crash. The input that triggered it is the
//! minimum reproducer the libfuzzer-shrinker has produced and the
//! panic message includes the v1/v2 ASTs and intermediate strings.
//!
//! # Layout note
//!
//! Mirrors `parser_roundtrip_oracle/mod.rs`: lives in a subdirectory
//! so Cargo's integration-test discovery skips it, then is brought
//! in by both the static-fixture driver
//! (`tests/via_roundtrip.rs`) and the fuzz target
//! (`fuzz/fuzz_targets/sip_via_typed_roundtrip.rs`) via `#[path]`.

#![allow(dead_code)]

use rsiprtp::sip::parser::typed::Via;

/// Assert the round-trip fixed-point invariant on `bytes` interpreted
/// as a `Via` header value.
///
/// If `bytes` is not valid UTF-8, returns silently — the framing layer
/// rejects non-UTF-8 upstream and the typed-header parser is never
/// asked to handle it.
///
/// If the first parse fails, returns silently — round-trip is
/// undefined for inputs the parser rejects (the crash target's job).
///
/// On parse-success: serialize, re-parse, serialize, re-parse, and
/// assert that the two re-parses produce both string-equal and
/// AST-equal results.
pub fn assert_via_roundtrip_fixed_point(bytes: &[u8]) {
    let Ok(s) = std::str::from_utf8(bytes) else {
        return;
    };
    let Ok(v1) = Via::parse(s) else {
        return;
    };
    let s2 = v1.to_string();
    let v2 = Via::parse(&s2).unwrap_or_else(|e| {
        panic!(
            "second parse failed: serializer produced text our parser \
             cannot accept.\nv1: {v1:#?}\ns2: {s2:?}\nerror: {e:?}",
        )
    });
    let s3 = v2.to_string();
    let v3 = Via::parse(&s3).unwrap_or_else(|e| {
        panic!(
            "third parse failed: round-trip not idempotent at parse step.\n\
             v2: {v2:#?}\ns3: {s3:?}\nerror: {e:?}",
        )
    });
    assert_eq!(
        v2, v3,
        "round-trip not a fixed point at v2 (AST inequality).\n\
         s2: {s2:?}\ns3: {s3:?}",
    );
    assert_eq!(
        s2, s3,
        "round-trip not a fixed point at v2 (string inequality).\n\
         s2: {s2:?}\ns3: {s3:?}",
    );
}
