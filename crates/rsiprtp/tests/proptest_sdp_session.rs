//! Property-based round-trip oracle driver for SDP session
//! descriptions: proptest generates structurally-valid
//! `SessionDescription` values via `SdpBuilder` (with surgical post-
//! `.build()` mutation for fields the builder does not expose) and
//! feeds the rendered bytes to the existing fixed-point oracle.
//!
//! See `wrk_docs/2026.05.06 - HLD - proptest SDP session.md` for the
//! full design. Track C — drives the same `assert_roundtrip_fixed_point`
//! oracle that the static fixture corpus (`tests/sdp_roundtrip.rs`) and
//! the libfuzzer target (`fuzz/fuzz_targets/sdp_session_roundtrip.rs`)
//! use, via the shared `#[path]` import.
//!
//! The complementary niche this driver fills vs. the libfuzzer target:
//! libfuzzer mutates raw bytes and overwhelmingly produces inputs that
//! fail the *first* parse (the oracle no-ops on those). Proptest stays
//! inside the lossless valid subset, so every generated case actually
//! exercises the full parse → serialize → re-parse → re-serialize
//! pipeline. The `prop_assert!` sanity check below is what keeps that
//! contract honest: a generator bug that produces unparseable bytes
//! must surface, not silently no-op.

#[path = "sdp_roundtrip_oracle/mod.rs"]
mod oracle;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use proptest::prelude::*;
use rsiprtp::sdp::{
    Attribute, Direction, MediaBuilder, MediaDescription, MediaType, SdpBuilder,
    SessionDescription, Timing,
};

// ---------------------------------------------------------------
// Origin (o=) and connection (c=) primitives
// ---------------------------------------------------------------

/// RFC 4566 username token, restricted to a subset that
/// `split_whitespace` cannot split. Never empty (the parser's
/// position-based origin parse would reassign fields if a field were
/// blank).
fn arb_origin_username() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_.\\-]{1,12}"
}

/// IPv4 — small curated set; proptest shrinks toward `0.0.0.0`
/// (the first `Just` arm). Nothing in the round-trip semantics depends
/// on the exact octets, so we keep failure messages readable.
fn arb_ip4() -> impl Strategy<Value = Ipv4Addr> {
    prop_oneof![
        Just(Ipv4Addr::new(0, 0, 0, 0)),
        Just(Ipv4Addr::LOCALHOST),
        Just(Ipv4Addr::new(192, 168, 1, 1)),
        Just(Ipv4Addr::new(10, 0, 0, 1)),
        Just(Ipv4Addr::new(203, 0, 113, 1)),
    ]
}

/// IPv6 — three representatives. Zone-id forms (`fe80::1%eth0`) are
/// out of scope per HLD §9.3.
fn arb_ip6() -> impl Strategy<Value = Ipv6Addr> {
    prop_oneof![
        Just(Ipv6Addr::UNSPECIFIED),
        Just(Ipv6Addr::LOCALHOST),
        Just(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
    ]
}

fn arb_ip() -> impl Strategy<Value = IpAddr> {
    prop_oneof![
        arb_ip4().prop_map(IpAddr::V4),
        arb_ip6().prop_map(IpAddr::V6),
    ]
}

// ---------------------------------------------------------------
// Session name (s=)
// ---------------------------------------------------------------

/// Session name. Always non-empty (HLD §3.2 decision (A)): the builder
/// cannot omit `s=`, and the missing-`s=` collapse is covered by the
/// static fixture `rt_sdp_missing_session_name`.
fn arb_session_name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("-".to_string()),
        Just("Test Session".to_string()),
        "[A-Za-z0-9 _.\\-]{1,20}",
    ]
}

// ---------------------------------------------------------------
// Timing (t=)
// ---------------------------------------------------------------

/// Bounded numeric timing. Stays inside the lossless u64 numeric subset
/// the oracle's "lossy timing values" entry calls out — generating
/// `u64::MAX` would render fine but bloat failure messages.
fn arb_timing() -> impl Strategy<Value = (u64, u64)> {
    (0u64..=2_000_000_000, 0u64..=2_000_000_000)
}

// ---------------------------------------------------------------
// Media (m=) — type, protocol, port, payload-type
// ---------------------------------------------------------------

/// Media type, excluding `MediaType::Other` (which is a known lossy
/// collapse — covered by static fixture `rt_sdp_media_type_other`).
fn arb_media_type() -> impl Strategy<Value = MediaType> {
    prop_oneof![
        Just(MediaType::Audio),
        Just(MediaType::Video),
        Just(MediaType::Application),
        Just(MediaType::Message),
    ]
}

/// RTP profile string. Only RTP/* protocols — non-RTP `m=` lines are
/// out of scope per HLD anti-scope.
fn arb_protocol() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("RTP/AVP".to_string()),
        Just("RTP/SAVP".to_string()),
        Just("RTP/AVPF".to_string()),
        Just("RTP/SAVPF".to_string()),
    ]
}

/// Static (RFC 3551 0..=34) and dynamic (96..=127) RTP payload-type
/// ranges. The parser does not enforce this — we just keep generated
/// PTs semantically sane.
fn arb_payload_type() -> impl Strategy<Value = u8> {
    prop_oneof![0u8..=34, 96u8..=127]
}

/// Any u16 port. Port 0 is the "rejected media" sentinel which round-
/// trips losslessly.
fn arb_port() -> impl Strategy<Value = u16> {
    any::<u16>()
}

// ---------------------------------------------------------------
// Direction (a=sendrecv|sendonly|recvonly|inactive)
// ---------------------------------------------------------------

fn arb_direction() -> impl Strategy<Value = Direction> {
    prop_oneof![
        Just(Direction::SendRecv),
        Just(Direction::SendOnly),
        Just(Direction::RecvOnly),
        Just(Direction::Inactive),
    ]
}

// ---------------------------------------------------------------
// Bandwidth (b=)
// ---------------------------------------------------------------

fn arb_bandwidth_modifier() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("AS".to_string()),
        Just("CT".to_string()),
        Just("RR".to_string()),
        Just("RS".to_string()),
        Just("TIAS".to_string()),
    ]
}

/// `prop::collection::hash_map` dedupes on key automatically, so the
/// resulting map has at most one entry per modifier.
fn arb_bandwidth() -> impl Strategy<Value = HashMap<String, u32>> {
    prop::collection::hash_map(arb_bandwidth_modifier(), 0u32..=10_000_000, 0..=5)
}

// ---------------------------------------------------------------
// ICE attribute pair (a=ice-ufrag, a=ice-pwd) — optional block
// ---------------------------------------------------------------

fn arb_ice_attrs() -> impl Strategy<Value = Vec<Attribute>> {
    prop_oneof![
        Just(Vec::<Attribute>::new()),
        ("[A-Za-z0-9]{4,8}", "[A-Za-z0-9]{22,40}").prop_map(|(u, p)| vec![
            Attribute {
                name: "ice-ufrag".to_string(),
                value: Some(u),
            },
            Attribute {
                name: "ice-pwd".to_string(),
                value: Some(p),
            },
        ]),
    ]
}

// ---------------------------------------------------------------
// Other attributes — conservative hand-picked allowlist per HLD §3.7
// supervisor sign-off Q1.
//
// TODO(HLD §9.1): if a real finding lands and this allowlist masks it,
// reconsider regex-with-denylist. The risk is that a future typed-
// parser change could make currently-untyped names fail-parse, which
// would silently no-op the oracle. For now we accept the coverage
// loss in exchange for shrink-friendliness (no `prop_filter`).
// ---------------------------------------------------------------

fn arb_other_attr() -> impl Strategy<Value = Attribute> {
    prop_oneof![
        Just(Attribute {
            name: "ptime".into(),
            value: Some("20".into()),
        }),
        Just(Attribute {
            name: "maxptime".into(),
            value: Some("40".into()),
        }),
        Just(Attribute {
            name: "rtcp-mux".into(),
            value: None,
        }),
        Just(Attribute {
            name: "label".into(),
            value: Some("audio0".into()),
        }),
    ]
}

// ---------------------------------------------------------------
// MediaBuilder composition
//
// `MediaBuilder::audio(port)` is the only public constructor. To
// generate non-audio media we build via `audio`, then overwrite
// `media_type` on the built `MediaDescription` (whose fields are all
// `pub`). This is cheaper than threading a separate constructor and
// keeps the path-(a) builder contract intact for everything else
// (formats, rtpmap/fmtp emission, direction attribute, defaults).
// ---------------------------------------------------------------

/// Build one `MediaDescription`. Returns the built description (not a
/// `MediaBuilder`) because we need to splice in fields the builder
/// does not expose: `media_type`, `bandwidth`, `connection`, and
/// extra attributes.
fn arb_media_description() -> impl Strategy<Value = MediaDescription> {
    (
        arb_media_type(),
        arb_port(),
        arb_protocol(),
        arb_direction(),
        prop::collection::vec(arb_payload_type(), 1..=4),
        arb_bandwidth(),
        prop::option::of(arb_ip()),
        prop::collection::vec(arb_other_attr(), 0..=3),
    )
        .prop_map(
            |(media_type, port, proto, dir, pts, bandwidth, conn_ip, extra_attrs)| {
                let mut mb = MediaBuilder::audio(port).protocol(proto).direction(dir);
                // Attach a codec for each PT. For static-PT 0 / 8 / 9 we
                // could use the convenience helpers, but `codec()` is
                // uniform and renders identically (rtpmap with rate
                // 8000, no channels) for these PTs.
                for &pt in &pts {
                    let (enc, rate, channels) = encoding_for_pt(pt);
                    mb = mb.codec(pt, enc, rate, channels);
                }
                let mut md = mb.build();
                // Path-(b) escape hatch: builder does not expose
                // media_type override (only `audio`), bandwidth,
                // media-level connection, or session-level attributes.
                md.media_type = media_type;
                md.bandwidth = bandwidth;
                if let Some(ip) = conn_ip {
                    md.connection = Some(rsiprtp::sdp::Connection {
                        net_type: "IN".to_string(),
                        addr_type: if ip.is_ipv4() { "IP4" } else { "IP6" }.to_string(),
                        address: ip.to_string(),
                    });
                }
                // The builder appended a direction attribute last; we
                // just append our extras after it. The parser stores
                // attributes in a `Vec` and serialization is order-
                // preserving, so any order reaches a fixed point. No
                // need to splice before the direction attribute.
                md.attributes.extend(extra_attrs);
                md
            },
        )
}

/// Stable encoding/rate/channels for any payload type the generator
/// emits. Static PTs (0, 8, 9) get their canonical RFC 3551 encoding;
/// other static PTs and all dynamic PTs get a synthetic but well-formed
/// rtpmap value that round-trips byte-for-byte.
fn encoding_for_pt(pt: u8) -> (&'static str, u32, Option<u8>) {
    match pt {
        0 => ("PCMU", 8000, None),
        8 => ("PCMA", 8000, None),
        9 => ("G722", 8000, None),
        _ if pt <= 34 => ("CELP", 8000, None),
        _ => ("opus", 48000, Some(2)),
    }
}

// ---------------------------------------------------------------
// SessionDescription composition
// ---------------------------------------------------------------

fn arb_session_description() -> impl Strategy<Value = SessionDescription> {
    (
        arb_origin_username(),
        any::<u64>(),
        any::<u64>(),
        arb_ip(),
        arb_session_name(),
        arb_timing(),
        prop::collection::vec(arb_media_description(), 0..=3),
        arb_ice_attrs(),
    )
        .prop_map(|(user, sid, sver, addr, name, (start, stop), media, ice)| {
            let mut sdp = SdpBuilder::new(addr)
                .username(user)
                .session_id(sid)
                .session_version(sver)
                .session_name(name)
                .build();
            sdp.timing = Timing { start, stop };
            sdp.media = media;
            // Attach optional ICE attrs to the first media if any
            // exists; otherwise drop them (session-level ICE is a
            // separate concern we don't model here — see HLD anti-
            // scope on full ICE combinatorics).
            if let Some(first) = sdp.media.first_mut() {
                first.attributes.extend(ice);
            }
            sdp
        })
}

// ---------------------------------------------------------------
// Properties
// ---------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Any builder-constructible (with documented escape-hatch
    /// mutations) `SessionDescription` round-trips at the s2 fixed
    /// point.
    #[test]
    fn proptest_sdp_session_roundtrips(sdp in arb_session_description()) {
        let bytes = sdp.to_string().into_bytes();
        // Sanity: generator must produce parseable SDP. Without this
        // assertion an oracle no-op (parse-fail short-circuit) would
        // silently pass the case, hiding generator bugs. Per HLD §4.
        prop_assert!(
            SessionDescription::parse(std::str::from_utf8(&bytes).unwrap()).is_ok(),
            "generator produced unparseable SDP:\n{}",
            String::from_utf8_lossy(&bytes),
        );
        oracle::assert_roundtrip_fixed_point(&bytes);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Sanity check (HLD §8 step 2, equivalent of the static driver's
    /// `rt_sdp_oracle_holds_on_canonical_input`): the bare minimum
    /// `SdpBuilder::new(any_ip).build()` round-trips. Protects against
    /// accidentally weakening the oracle.
    #[test]
    fn proptest_sdp_min_canonical_round_trips(addr in arb_ip()) {
        let bytes = SdpBuilder::new(addr).build().to_string().into_bytes();
        prop_assert!(
            SessionDescription::parse(std::str::from_utf8(&bytes).unwrap()).is_ok(),
            "minimal builder produced unparseable SDP:\n{}",
            String::from_utf8_lossy(&bytes),
        );
        oracle::assert_roundtrip_fixed_point(&bytes);
    }
}
