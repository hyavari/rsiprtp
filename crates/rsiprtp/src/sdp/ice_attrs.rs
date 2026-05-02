//! ICE-related SDP attribute helpers (RFC 8839).
//!
//! Read/write helpers for `a=ice-ufrag`, `a=ice-pwd`, `a=candidate:`, and
//! `a=rtcp-mux` against the existing generic `Attribute` API, plus an
//! `apply_default_candidate` helper that patches the `c=` connection
//! address and `m=` port to the default candidate's address.

use crate::ice::{Candidate, CandidateType};
use crate::sdp::parser::{Attribute, Connection, MediaDescription, SessionDescription};

/// Read `a=ice-ufrag` and `a=ice-pwd` from a media description.
///
/// Returns `Some((ufrag, pwd))` only if both attributes are present.
pub fn read_ice_credentials(m: &MediaDescription) -> Option<(String, String)> {
    let ufrag = attr_value(m, "ice-ufrag")?;
    let pwd = attr_value(m, "ice-pwd")?;
    Some((ufrag, pwd))
}

/// Read all `a=candidate:` attributes from a media description.
///
/// Lines that fail to parse are skipped (and logged at debug level).
pub fn read_candidates(m: &MediaDescription) -> Vec<Candidate> {
    m.attributes
        .iter()
        .filter(|a| a.name == "candidate")
        .filter_map(|a| {
            let v = a.value.as_deref()?;
            match Candidate::from_sdp(v) {
                Some(c) => Some(c),
                None => {
                    tracing::debug!(value = %v, "ICE candidate parse failed");
                    None
                }
            }
        })
        .collect()
}

/// Return true iff `a=rtcp-mux` is present on the media description.
pub fn read_rtcp_mux(m: &MediaDescription) -> bool {
    m.attributes.iter().any(|a| a.name == "rtcp-mux")
}

/// Write `a=ice-ufrag` and `a=ice-pwd` onto a media description.
///
/// Replaces any existing values for those attributes.
pub fn write_ice_credentials(m: &mut MediaDescription, ufrag: &str, pwd: &str) {
    m.attributes
        .retain(|a| a.name != "ice-ufrag" && a.name != "ice-pwd");
    m.attributes.push(Attribute {
        name: "ice-ufrag".to_string(),
        value: Some(ufrag.to_string()),
    });
    m.attributes.push(Attribute {
        name: "ice-pwd".to_string(),
        value: Some(pwd.to_string()),
    });
}

/// Write `a=candidate:` lines for each candidate.
///
/// Existing candidate attributes are removed first.
pub fn write_candidates(m: &mut MediaDescription, cands: &[Candidate]) {
    m.attributes.retain(|a| a.name != "candidate");
    for c in cands {
        m.attributes.push(Attribute {
            name: "candidate".to_string(),
            value: Some(c.to_sdp()),
        });
    }
}

/// Write a single `a=rtcp-mux` flag attribute (idempotent).
pub fn write_rtcp_mux(m: &mut MediaDescription) {
    if !read_rtcp_mux(m) {
        m.attributes.push(Attribute {
            name: "rtcp-mux".to_string(),
            value: None,
        });
    }
}

/// Patch the media-level `c=` connection line and `m=` port on a media
/// description so they match the default candidate's address (RFC 8839
/// §4.3.1).
///
/// Per RFC 8839 §4.3.1 the default address is *media-level*: the
/// session-level `c=` line is intentionally not touched, so multi-m-line
/// SDP with mixed ICE/non-ICE streams isn't polluted.
///
/// # Caller contract
///
/// `default` must be a host candidate. Phase 2's
/// `IceSession::default_candidate()` is responsible for the choice
/// (lowest-priority host candidate, or first host if no others exist).
/// This function does not validate or pick — it just applies what the
/// caller chose.
///
/// # Panics
///
/// Panics if `media_index` is out of bounds. A bad index is a programming
/// error; a debug-build `debug_assert!` makes it loud in tests, and
/// release builds still panic via `expect`.
pub fn apply_default_candidate(
    sdp: &mut SessionDescription,
    media_index: usize,
    default: &Candidate,
) {
    debug_assert!(
        media_index < sdp.media.len(),
        "apply_default_candidate: media_index {} out of bounds (len {})",
        media_index,
        sdp.media.len()
    );
    debug_assert!(
        default.candidate_type == CandidateType::Host,
        "apply_default_candidate: default must be a host candidate (got {:?})",
        default.candidate_type
    );

    let addr_type = if default.address.is_ipv4() {
        "IP4"
    } else {
        "IP6"
    };
    let address = default.address.ip().to_string();

    let media = sdp
        .media
        .get_mut(media_index)
        .expect("apply_default_candidate: media_index out of bounds");
    media.port = default.address.port();
    if let Some(conn) = media.connection.as_mut() {
        conn.addr_type = addr_type.to_string();
        conn.address = address;
    } else {
        media.connection = Some(Connection {
            net_type: "IN".to_string(),
            addr_type: addr_type.to_string(),
            address,
        });
    }
}

fn attr_value(m: &MediaDescription, name: &str) -> Option<String> {
    m.attributes
        .iter()
        .find(|a| a.name == name)
        .and_then(|a| a.value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdp::parser::{MediaType, SessionDescription};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    fn audio_media() -> MediaDescription {
        MediaDescription {
            media_type: MediaType::Audio,
            port: 49170,
            num_ports: None,
            protocol: "RTP/AVP".to_string(),
            formats: vec!["0".to_string()],
            connection: None,
            bandwidth: Default::default(),
            attributes: Vec::new(),
        }
    }

    #[test]
    fn test_read_credentials_missing() {
        let m = audio_media();
        assert!(read_ice_credentials(&m).is_none());
    }

    #[test]
    fn test_read_credentials_partial() {
        let mut m = audio_media();
        m.attributes.push(Attribute {
            name: "ice-ufrag".to_string(),
            value: Some("abc12345".to_string()),
        });
        // Missing ice-pwd
        assert!(read_ice_credentials(&m).is_none());
    }

    #[test]
    fn test_credentials_round_trip() {
        let mut m = audio_media();
        write_ice_credentials(&mut m, "abc12345", "supersecretpassword12345");
        let (u, p) = read_ice_credentials(&m).expect("credentials");
        assert_eq!(u, "abc12345");
        assert_eq!(p, "supersecretpassword12345");
    }

    #[test]
    fn test_credentials_overwrite() {
        let mut m = audio_media();
        write_ice_credentials(&mut m, "old", "oldpwd");
        write_ice_credentials(&mut m, "new", "newpwd");
        let (u, p) = read_ice_credentials(&m).unwrap();
        assert_eq!(u, "new");
        assert_eq!(p, "newpwd");
        assert_eq!(
            m.attributes
                .iter()
                .filter(|a| a.name == "ice-ufrag")
                .count(),
            1
        );
        assert_eq!(
            m.attributes.iter().filter(|a| a.name == "ice-pwd").count(),
            1
        );
    }

    #[test]
    fn test_candidates_round_trip() {
        let mut m = audio_media();
        let host = Candidate::host(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 5004),
            1,
        );
        let srflx = Candidate::server_reflexive(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)), 12345),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 5004),
            1,
        );
        let cands = vec![host.clone(), srflx.clone()];
        write_candidates(&mut m, &cands);
        let read = read_candidates(&m);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0], host);
        assert_eq!(read[1], srflx);
    }

    #[test]
    fn test_candidates_overwrite() {
        let mut m = audio_media();
        let c1 = Candidate::host(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 5000),
            1,
        );
        let c2 = Candidate::host(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), 5002),
            1,
        );
        write_candidates(&mut m, std::slice::from_ref(&c1));
        write_candidates(&mut m, std::slice::from_ref(&c2));
        let read = read_candidates(&m);
        assert_eq!(read, vec![c2]);
    }

    #[test]
    fn test_unparseable_candidate_skipped() {
        let mut m = audio_media();
        m.attributes.push(Attribute {
            name: "candidate".to_string(),
            value: Some("garbage data".to_string()),
        });
        let host = Candidate::host(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1)), 5004),
            1,
        );
        m.attributes.push(Attribute {
            name: "candidate".to_string(),
            value: Some(host.to_sdp()),
        });
        let read = read_candidates(&m);
        assert_eq!(read, vec![host]);
    }

    #[test]
    fn test_rtcp_mux_round_trip() {
        let mut m = audio_media();
        assert!(!read_rtcp_mux(&m));
        write_rtcp_mux(&mut m);
        assert!(read_rtcp_mux(&m));
    }

    #[test]
    fn test_rtcp_mux_idempotent() {
        let mut m = audio_media();
        write_rtcp_mux(&mut m);
        write_rtcp_mux(&mut m);
        assert_eq!(
            m.attributes.iter().filter(|a| a.name == "rtcp-mux").count(),
            1
        );
    }

    #[test]
    fn test_full_round_trip_through_parser() {
        // Build an SDP with ICE attrs, render to text, parse back, read attrs.
        let sdp_text = r#"v=0
o=- 123 1 IN IP4 192.168.1.1
s=-
c=IN IP4 192.168.1.1
t=0 0
m=audio 49170 RTP/AVP 0
a=rtpmap:0 PCMU/8000
"#;
        let mut sdp = SessionDescription::parse(sdp_text).unwrap();
        let host = Candidate::host(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 5004),
            1,
        );
        let media = sdp.audio_media_mut().unwrap();
        write_ice_credentials(media, "abcd1234", "0123456789abcdef01234567");
        write_candidates(media, std::slice::from_ref(&host));
        write_rtcp_mux(media);

        // Manually serialise the m=/a= block and re-parse — we don't have a
        // builder helper for ICE attrs, so go via the existing builder shape.
        let rendered = render_for_test(&sdp);
        let parsed = SessionDescription::parse(&rendered).expect("re-parse");
        let media = parsed.audio_media().unwrap();
        let (u, p) = read_ice_credentials(media).expect("credentials");
        assert_eq!(u, "abcd1234");
        assert_eq!(p, "0123456789abcdef01234567");
        assert_eq!(read_candidates(media), vec![host]);
        assert!(read_rtcp_mux(media));
    }

    #[test]
    fn test_apply_default_candidate_ipv4() {
        let sdp_text = r#"v=0
o=- 123 1 IN IP4 192.168.1.1
s=-
c=IN IP4 192.168.1.1
t=0 0
m=audio 49170 RTP/AVP 0
a=rtpmap:0 PCMU/8000
"#;
        let mut sdp = SessionDescription::parse(sdp_text).unwrap();
        let host = Candidate::host(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 7000),
            1,
        );
        apply_default_candidate(&mut sdp, 0, &host);

        let media = sdp.audio_media().unwrap();
        assert_eq!(media.port, 7000);
        let mconn = media.connection.as_ref().expect("media-level c=");
        assert_eq!(mconn.addr_type, "IP4");
        assert_eq!(mconn.address, "10.0.0.5");
        // Session-level c= is intentionally untouched (RFC 8839 §4.3.1
        // is media-level).
        let conn = sdp.connection.as_ref().unwrap();
        assert_eq!(conn.address, "192.168.1.1");
        assert_eq!(conn.addr_type, "IP4");
    }

    #[test]
    fn test_apply_default_candidate_ipv6() {
        let sdp_text = r#"v=0
o=- 123 1 IN IP6 ::1
s=-
c=IN IP6 ::1
t=0 0
m=audio 49170 RTP/AVP 0
a=rtpmap:0 PCMU/8000
"#;
        let mut sdp = SessionDescription::parse(sdp_text).unwrap();
        let host = Candidate::host(
            SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
                7100,
            ),
            1,
        );
        apply_default_candidate(&mut sdp, 0, &host);

        let media = sdp.audio_media().unwrap();
        assert_eq!(media.port, 7100);
        let mconn = media.connection.as_ref().unwrap();
        assert_eq!(mconn.addr_type, "IP6");
        assert_eq!(mconn.address, "2001:db8::1");
        // Session-level c= is left as-is.
        let conn = sdp.connection.as_ref().unwrap();
        assert_eq!(conn.addr_type, "IP6");
        assert_eq!(conn.address, "::1");
    }

    #[test]
    fn test_apply_default_candidate_does_not_pollute_other_media() {
        // Multi-m-line SDP: only the targeted m= line should be patched.
        let sdp_text = r#"v=0
o=- 123 1 IN IP4 192.168.1.1
s=-
c=IN IP4 192.168.1.1
t=0 0
m=audio 49170 RTP/AVP 0
a=rtpmap:0 PCMU/8000
m=video 49180 RTP/AVP 96
a=rtpmap:96 H264/90000
"#;
        let mut sdp = SessionDescription::parse(sdp_text).unwrap();
        let host = Candidate::host(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 7000),
            1,
        );
        apply_default_candidate(&mut sdp, 0, &host);

        // Audio (index 0) was patched.
        assert_eq!(sdp.media[0].port, 7000);
        let aconn = sdp.media[0].connection.as_ref().unwrap();
        assert_eq!(aconn.address, "10.0.0.5");

        // Video (index 1) is untouched.
        assert_eq!(sdp.media[1].port, 49180);
        assert!(sdp.media[1].connection.is_none());

        // Session-level c= is untouched.
        let conn = sdp.connection.as_ref().unwrap();
        assert_eq!(conn.address, "192.168.1.1");
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn test_apply_default_candidate_bad_index_panics() {
        let sdp_text = r#"v=0
o=- 123 1 IN IP4 192.168.1.1
s=-
c=IN IP4 192.168.1.1
t=0 0
m=audio 49170 RTP/AVP 0
a=rtpmap:0 PCMU/8000
"#;
        let mut sdp = SessionDescription::parse(sdp_text).unwrap();
        let host = Candidate::host(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 7000),
            1,
        );
        apply_default_candidate(&mut sdp, 99, &host);
    }

    /// Render the SDP back to text well enough for round-trip tests.
    ///
    /// `SdpBuilder` doesn't expose ICE attrs, so we render manually here.
    fn render_for_test(sdp: &SessionDescription) -> String {
        let mut out = String::new();
        out.push_str(&format!("v={}\r\n", sdp.version));
        let o = &sdp.origin;
        out.push_str(&format!(
            "o={} {} {} {} {} {}\r\n",
            o.username, o.session_id, o.session_version, o.net_type, o.addr_type, o.unicast_address
        ));
        out.push_str(&format!("s={}\r\n", sdp.session_name));
        if let Some(c) = &sdp.connection {
            out.push_str(&format!(
                "c={} {} {}\r\n",
                c.net_type, c.addr_type, c.address
            ));
        }
        out.push_str(&format!("t={} {}\r\n", sdp.timing.start, sdp.timing.stop));
        for m in &sdp.media {
            let media_str = match m.media_type {
                MediaType::Audio => "audio",
                MediaType::Video => "video",
                MediaType::Application => "application",
                MediaType::Message => "message",
                MediaType::Other => "other",
            };
            out.push_str(&format!(
                "m={} {} {} {}\r\n",
                media_str,
                m.port,
                m.protocol,
                m.formats.join(" ")
            ));
            if let Some(c) = &m.connection {
                out.push_str(&format!(
                    "c={} {} {}\r\n",
                    c.net_type, c.addr_type, c.address
                ));
            }
            for a in &m.attributes {
                if let Some(v) = &a.value {
                    out.push_str(&format!("a={}:{}\r\n", a.name, v));
                } else {
                    out.push_str(&format!("a={}\r\n", a.name));
                }
            }
        }
        out
    }
}
