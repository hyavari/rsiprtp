//! Property-based round-trip oracle driver: proptest generates
//! structurally-valid SIP requests and responses as canonical CRLF
//! wire bytes and feeds them to the existing Tier-1 fixed-point
//! oracle.
//!
//! See `wrk_docs/2026.05.06 - HLD - proptest SIP message.md` for the
//! full design. Track A — Tier-1 (`Message::parse ∘ Message::to_bytes`)
//! only. Tier-2 typed-header round-trip belongs to Track B.
//!
//! The oracle module at `parser_roundtrip_oracle/mod.rs` is reused
//! verbatim via `#[path]`, exactly as `parser_roundtrip.rs` does.
//!
//! # Soak knob
//!
//! Set `RSIPRTP_PROPTEST_CASES=N` to override the default of 256
//! cases. Per-track env var (rather than proptest's global
//! `PROPTEST_CASES`) keeps Tracks B and C independent.
//!
//! # Shrinking caveat
//!
//! Proptest's component-wise shrinker may shrink interdependent
//! choices independently — e.g. if a failure depends on (header value
//! contains some character, status code is N) the shrinker can shrink
//! one without the other and lose the failure. When that happens
//! consult `proptest-regressions/` (the persistent regressions file)
//! for the original failing seed and rerun with
//! `PROPTEST_MAX_SHRINK_ITERS=0`.

#[path = "parser_roundtrip_oracle/mod.rs"]
mod oracle;

use proptest::prelude::*;
use rsiprtp::sip::parser::Header;

// ---------------------------------------------------------------
// Method
// ---------------------------------------------------------------

/// Method token. 90% recognized methods, 10% case-variant of a
/// recognized method (parser is case-insensitive; serializer
/// canonicalizes to uppercase, so m1 ≠ m2 in bytes — fixed point at
/// m2 holds). Per HLD §2.1 supervisor sign-off Q1: keep the
/// case-variant tail.
fn arb_method_token() -> impl Strategy<Value = String> {
    prop_oneof![
        18 => prop::sample::select(&[
            "INVITE", "ACK", "BYE", "CANCEL", "REGISTER", "OPTIONS",
            "INFO", "UPDATE", "REFER", "NOTIFY", "SUBSCRIBE", "PRACK",
            "MESSAGE", "PUBLISH",
        ]).prop_map(str::to_owned),
        1 => prop::sample::select(&["invite", "Invite", "iNvItE", "bye", "publish"])
            .prop_map(str::to_owned),
    ]
}

// ---------------------------------------------------------------
// Request-URI components (constrained to what `SipUri::parse` accepts)
// ---------------------------------------------------------------

// NOTE: bare regex string literals (e.g. `"[a-z]+"`) implement
// `Strategy<Value = String>` directly via proptest's default `regex`
// feature. Only `prop::sample::select(&[&str])` needs an explicit
// `.prop_map(str::to_owned)` to lift `&'static str` to `String`.
//
// In `prop_oneof!` arms we coerce mixed strategy shapes to
// `BoxedStrategy<String>` via `.boxed()` so all arms have the same type.

fn arb_user() -> BoxedStrategy<String> {
    "[a-zA-Z0-9_.~+-]{1,12}".boxed()
}

fn arb_host() -> BoxedStrategy<String> {
    prop_oneof![
        4 => "[a-z][a-z0-9-]{1,8}(\\.[a-z][a-z0-9-]{1,8}){0,3}".boxed(),
        2 => (0u8..=255, 0u8..=255, 0u8..=255, 0u8..=255)
            .prop_map(|(a, b, c, d)| format!("{a}.{b}.{c}.{d}")).boxed(),
        1 => prop::sample::select(&["[::1]", "[2001:db8::1]", "[fe80::1]"])
            .prop_map(str::to_owned).boxed(),
    ]
    .boxed()
}

fn arb_port() -> impl Strategy<Value = Option<u16>> {
    prop::option::weighted(0.3, 1u16..=65535)
}

fn arb_request_uri() -> impl Strategy<Value = String> {
    prop_oneof![
        7 => (arb_user(), arb_host(), arb_port())
            .prop_map(|(u, h, p)| match p {
                Some(port) => format!("sip:{u}@{h}:{port}"),
                None => format!("sip:{u}@{h}"),
            }),
        2 => (arb_user(), arb_host(), arb_port())
            .prop_map(|(u, h, p)| match p {
                Some(port) => format!("sips:{u}@{h}:{port}"),
                None => format!("sips:{u}@{h}"),
            }),
        1 => arb_host().prop_map(|h| format!("sip:{h}")),
        1 => "(\\+?[1-9][0-9]{6,14})".prop_map(|n| format!("tel:{n}")),
    ]
}

// ---------------------------------------------------------------
// Status code & reason phrase
// ---------------------------------------------------------------

fn arb_status_code() -> impl Strategy<Value = u16> {
    prop_oneof![
        4 => 100u16..=699,
        1 => prop::sample::select(&[
            100u16, 180, 183, 200, 202, 301, 302, 401, 403, 404,
            407, 408, 480, 486, 487, 488, 500, 503, 504, 603,
        ]),
    ]
}

/// Reason phrase. Restricted to bytes the parser accepts at the
/// status-line layer (see `framing::parse_status_line`: rejects DEL
/// and `< 0x20` other than HTAB).
///
/// Per HLD §2.3 supervisor sign-off Q2: include a small high-bit
/// UTF-8 tail to exercise the parser's UTF8-NONASCII / UTF8-CONT
/// branch (`framing::test_parse_status_line_reason_with_high_bit_accepts`).
/// Curated to a tiny set of printable Latin-1 / Greek letters: all
/// render as a single grapheme in failure messages (no
/// `\u{XXXX}` escape sequences), no control chars, no surrogates.
fn arb_reason_phrase() -> BoxedStrategy<String> {
    prop_oneof![
        4 => ("[A-Za-z0-9 ]{0,32}").boxed(),
        1 => Just(String::new()).boxed(),
        1 => Just("Busy Here".to_string()).boxed(),
        1 => Just("OK".to_string()).boxed(),
        1 => ("[A-Za-z0-9\t ]{0,32}").boxed(),
        1 => prop::sample::select(&[
            "café", "naïve", "Ω OK", "résumé", "Grüße", "señor",
        ]).prop_map(str::to_owned).boxed(),
    ]
    .boxed()
}

// ---------------------------------------------------------------
// Headers
// ---------------------------------------------------------------

/// Curated literal values for the 19 native `Header` variants. All
/// entries are known to round-trip cleanly at Tier-1 (the oracle
/// only checks `Message::parse ∘ Message::to_bytes`; Tier-2 typed
/// validity is not asserted here — Track B's job).
///
/// `ContentLength` is intentionally absent: the assembler synthesizes
/// it from the actual body length so the wire is always consistent.
fn arb_native_header() -> impl Strategy<Value = Header> {
    let allow = prop::sample::select(&["INVITE, ACK", "INVITE, ACK, BYE, CANCEL", "INVITE"]);
    let auth = prop::sample::select(&[
        "Digest realm=\"x\", nonce=\"abc\"",
        "Digest username=\"a\", realm=\"x\", nonce=\"y\", uri=\"sip:a@b\", response=\"deadbeef\"",
    ]);
    let cseq = (
        1u32..=999_999,
        prop::sample::select(&[
            "INVITE",
            "ACK",
            "BYE",
            "CANCEL",
            "REGISTER",
            "OPTIONS",
            "INFO",
            "UPDATE",
            "REFER",
            "NOTIFY",
            "SUBSCRIBE",
            "PRACK",
            "MESSAGE",
            "PUBLISH",
        ]),
    )
        .prop_map(|(n, m)| format!("{n} {m}"));
    let call_id: BoxedStrategy<String> = "[a-zA-Z0-9]{8,16}@[a-z][a-z0-9.-]{2,12}".boxed();
    let contact = (arb_user(), arb_host()).prop_map(|(u, h)| format!("<sip:{u}@{h}>"));
    let content_type = prop::sample::select(&[
        "application/sdp",
        "text/plain",
        "application/dtmf-relay",
        "message/sipfrag",
        "application/pidf+xml",
    ]);
    let expires = (0u32..=86_400).prop_map(|n| n.to_string());
    // Two structurally identical strategies so prop_oneof! can consume
    // each independently (Strategy is not Clone for the tuple impl
    // returned by prop_map'd combinators here).
    let from_hdr = (arb_user(), arb_host(), "[a-f0-9]{6,12}")
        .prop_map(|(u, h, tag)| format!("<sip:{u}@{h}>;tag={tag}"));
    let to_hdr = (arb_user(), arb_host(), "[a-f0-9]{6,12}")
        .prop_map(|(u, h, tag)| format!("<sip:{u}@{h}>;tag={tag}"));
    let max_fwd = (0u8..=70).prop_map(|n| n.to_string());
    let record_route = (arb_user(), arb_host()).prop_map(|(u, h)| format!("<sip:{u}@{h};lr>"));
    let route = (arb_user(), arb_host()).prop_map(|(u, h)| format!("<sip:{u}@{h};lr>"));
    let require = prop::sample::select(&["100rel", "timer", "path", "100rel, timer"]);
    let supported = prop::sample::select(&["timer", "100rel", "path", "replaces", ""]);
    let via = (arb_host(), "[a-zA-Z0-9]{16,24}")
        .prop_map(|(h, b)| format!("SIP/2.0/UDP {h};branch=z9hG4bK{b}"));
    let www_auth = prop::sample::select(&["Digest realm=\"x\", nonce=\"y\""]);

    prop_oneof![
        2 => allow.prop_map(|s| Header::Allow(s.into())),
        2 => auth.prop_map(|s| Header::Authorization(s.into())),
        4 => cseq.prop_map(Header::CSeq),
        4 => call_id.prop_map(Header::CallId),
        4 => contact.prop_map(Header::Contact),
        3 => content_type.prop_map(|s| Header::ContentType(s.into())),
        2 => expires.prop_map(Header::Expires),
        4 => from_hdr.prop_map(Header::From),
        4 => to_hdr.prop_map(Header::To),
        3 => max_fwd.prop_map(Header::MaxForwards),
        2 => prop::sample::select(&["Digest"]).prop_map(|s| Header::ProxyAuthenticate(s.into())),
        2 => prop::sample::select(&["Digest"]).prop_map(|s| Header::ProxyAuthorization(s.into())),
        2 => record_route.prop_map(Header::RecordRoute),
        2 => require.prop_map(|s| Header::Require(s.into())),
        2 => route.prop_map(Header::Route),
        2 => supported.prop_map(|s| Header::Supported(s.into())),
        4 => via.prop_map(Header::Via),
        2 => www_auth.prop_map(|s| Header::WwwAuthenticate(s.into())),
    ]
}

/// Header name + value for `Header::Other`. The names listed here
/// either resolve to long-form names that are not in the 19 native
/// variants (so they land in `Other` on parse) or are `X-` prefixed.
///
/// Value strategy: Option A — structurally compose the value as
/// (non-ws-char, [printable-ASCII]{0,38}, non-ws-char) so the result
/// can never have leading or trailing whitespace. Avoids the
/// HLD-original `prop_filter` (Track B's HLD flagged filter overuse
/// as a shrink/budget anti-pattern). The interior class deliberately
/// admits ` ` and `\t` so the value can still embed whitespace —
/// only the edges are constrained. Empty value is generated as a
/// single non-whitespace character (the leading edge is the entire
/// value when the interior length shrinks to 0).
fn arb_other_header() -> impl Strategy<Value = Header> {
    let safe_name: BoxedStrategy<String> = prop_oneof![
        4 => prop::sample::select(&[
            "User-Agent", "Server", "Subject", "Date", "Organization",
            "Priority", "Reply-To", "Timestamp",
        ]).prop_map(str::to_owned).boxed(),
        3 => "X-[A-Z][A-Za-z]{1,8}".boxed(),
    ]
    .boxed();
    // Edge bytes: printable ASCII excluding SP and HTAB. Interior
    // alphabet is full printable ASCII + tab (the Tier-1 header
    // parser tolerates these per `parse_header_block`).
    let value = (
        "[!-~]",         // leading non-whitespace edge (single byte)
        "[\t -~]{0,38}", // interior, may include space/tab
        "[!-~]",         // trailing non-whitespace edge (single byte)
    )
        .prop_map(|(lead, mid, trail)| format!("{lead}{mid}{trail}"));
    (safe_name, value).prop_map(|(n, v)| Header::Other(n, v))
}

fn arb_header_set() -> impl Strategy<Value = Vec<Header>> {
    prop::collection::vec(
        prop_oneof![
            7 => arb_native_header(),
            3 => arb_other_header(),
        ],
        0..=12,
    )
}

// ---------------------------------------------------------------
// Body
// ---------------------------------------------------------------

fn arb_body() -> BoxedStrategy<Vec<u8>> {
    prop_oneof![
        4 => Just(Vec::new()).boxed(),
        3 => "[ -~]{1,200}".prop_map(|s: String| s.into_bytes()).boxed(),
        2 => prop::collection::vec(any::<u8>(), 1..=200).boxed(),
        1 => Just(b"v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=-\r\nc=IN IP4 127.0.0.1\r\nt=0 0\r\n".to_vec()).boxed(),
    ]
    .boxed()
}

// ---------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------

fn assemble_request(method: String, uri: String, headers: Vec<Header>, body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(256 + body.len());
    out.extend_from_slice(method.as_bytes());
    out.push(b' ');
    out.extend_from_slice(uri.as_bytes());
    out.push(b' ');
    out.extend_from_slice(b"SIP/2.0\r\n");
    write_headers_and_terminator(&mut out, &headers, body.len());
    out.extend_from_slice(&body);
    out
}

fn assemble_response(code: u16, reason: String, headers: Vec<Header>, body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(256 + body.len());
    out.extend_from_slice(b"SIP/2.0 ");
    out.extend_from_slice(code.to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(reason.as_bytes());
    out.extend_from_slice(b"\r\n");
    write_headers_and_terminator(&mut out, &headers, body.len());
    out.extend_from_slice(&body);
    out
}

/// Write the header block plus the synthesized Content-Length and
/// the blank-line terminator. Per HLD §2.5: always emit a fresh
/// Content-Length consistent with the body length so the property
/// reduces to "consistent input is a true fixed point at m1".
fn write_headers_and_terminator(out: &mut Vec<u8>, headers: &[Header], body_len: usize) {
    for h in headers {
        out.extend_from_slice(h.name().as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(h.value().as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"Content-Length: ");
    out.extend_from_slice(body_len.to_string().as_bytes());
    out.extend_from_slice(b"\r\n\r\n");
}

fn arb_request_bytes() -> impl Strategy<Value = Vec<u8>> {
    (
        arb_method_token(),
        arb_request_uri(),
        arb_header_set(),
        arb_body(),
    )
        .prop_map(|(m, u, h, b)| assemble_request(m, u, h, b))
}

fn arb_response_bytes() -> impl Strategy<Value = Vec<u8>> {
    (
        arb_status_code(),
        arb_reason_phrase(),
        arb_header_set(),
        arb_body(),
    )
        .prop_map(|(c, r, h, b)| assemble_response(c, r, h, b))
}

fn arb_message() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        1 => arb_request_bytes(),
        1 => arb_response_bytes(),
    ]
}

// ---------------------------------------------------------------
// Run shape
// ---------------------------------------------------------------

/// Soak knob. `RSIPRTP_PROPTEST_CASES=N` overrides the default 256.
/// Per-track env var (not proptest's global `PROPTEST_CASES`) keeps
/// Tracks B/C independent.
fn cases() -> u32 {
    std::env::var("RSIPRTP_PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: cases(),
        max_shrink_iters: 4096,
        max_global_rejects: 65_536,
        .. ProptestConfig::default()
    })]

    /// Tier-1 fixed-point round-trip on procedurally generated SIP
    /// requests and responses. The single assertion is the existing
    /// oracle; we deliberately do NOT compare against the input
    /// bytes (m1 may legitimately differ from input — case
    /// canonicalization, header reordering is NOT something the
    /// serializer does, but per-line whitespace trim and canonical
    /// header-name casing all happen on the first round-trip).
    #[test]
    fn rt_proptest_sip_message(bytes in arb_message()) {
        oracle::assert_roundtrip_fixed_point(&bytes);
    }
}
