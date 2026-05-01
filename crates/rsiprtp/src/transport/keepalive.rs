//! RFC 5626 §3.5.1 / §4.4.1 CRLF keep-alive for connection-oriented SIP
//! transports (TCP, TLS).
//!
//! Keep-alive messages on the wire:
//! - **Ping**: `\r\n\r\n` (sent unprompted to keep NAT pinholes / firewall
//!   state alive).
//! - **Pong**: `\r\n` (response to a ping).
//!
//! Without keep-alive, persistent TCP/TLS connections behind NAT will
//! silently rot once the NAT idle timer expires, with no error surface
//! — calls just stop arriving. CRLF keep-alive is cheap (4 bytes every
//! ~95s by default) and solves this class of bug entirely.
//!
//! This module exposes the byte-level primitives. The TCP and TLS
//! transports drive them from their own read loops and per-connection
//! send tasks.

use bytes::BytesMut;
use std::time::Duration;

/// Wire bytes for a SIP keep-alive ping (`CRLF CRLF`).
pub const KEEPALIVE_PING: &[u8] = b"\r\n\r\n";

/// Wire bytes for a SIP keep-alive pong (`CRLF`).
pub const KEEPALIVE_PONG: &[u8] = b"\r\n";

/// Default ping interval per RFC 5626 §4.4.1 (95 seconds).
pub const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(95);

/// Per-connection keep-alive configuration.
#[derive(Debug, Clone)]
pub struct KeepAliveConfig {
    /// If `true`, send periodic ping (`\r\n\r\n`) frames on outbound
    /// connections. Always reply to incoming pings regardless.
    pub send_pings: bool,
    /// Interval between outbound pings.
    pub ping_interval: Duration,
}

impl Default for KeepAliveConfig {
    fn default() -> Self {
        Self {
            send_pings: false,
            ping_interval: DEFAULT_PING_INTERVAL,
        }
    }
}

impl KeepAliveConfig {
    /// Enabled with the RFC 5626 default interval.
    pub fn enabled() -> Self {
        Self {
            send_pings: true,
            ..Default::default()
        }
    }

    /// Enabled with a custom interval.
    pub fn enabled_with_interval(interval: Duration) -> Self {
        Self {
            send_pings: true,
            ping_interval: interval,
        }
    }
}

/// Strip any run of leading `\r\n` pairs from `buf` and report how many
/// of them constitute keep-alive pings (each pair of CRLFs is one ping;
/// any odd trailing CRLF is consumed as a pong).
///
/// SIP framing per RFC 3261 §7.5 already permits leading whitespace
/// before a request, so consuming this prefix is always safe.
///
/// Returns the number of pings detected — the caller should send one
/// pong (`\r\n`) per ping back to the peer.
pub fn strip_leading_keepalives(buf: &mut BytesMut) -> usize {
    let mut crlf_count = 0usize;
    while buf.len() >= (crlf_count + 1) * 2 && &buf[crlf_count * 2..crlf_count * 2 + 2] == b"\r\n" {
        crlf_count += 1;
    }
    if crlf_count > 0 {
        let _ = buf.split_to(crlf_count * 2);
    }
    // Each pair of consecutive CRLFs is one ping; a trailing odd CRLF
    // is a pong (or the first half of a ping whose second CRLF will
    // arrive in a later read — also fine to consume since the caller
    // re-checks on every read).
    crlf_count / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(bytes: &[u8]) -> BytesMut {
        BytesMut::from(bytes)
    }

    #[test]
    fn empty_buffer_no_pings() {
        let mut b = buf(b"");
        assert_eq!(strip_leading_keepalives(&mut b), 0);
        assert_eq!(&b[..], b"");
    }

    #[test]
    fn no_leading_crlf_passes_through() {
        let mut b = buf(b"INVITE sip:bob@example.com SIP/2.0\r\n");
        assert_eq!(strip_leading_keepalives(&mut b), 0);
        assert_eq!(&b[..], b"INVITE sip:bob@example.com SIP/2.0\r\n");
    }

    #[test]
    fn single_crlf_is_pong_no_ping() {
        // A bare CRLF is a pong (or a partial ping awaiting more data).
        // Either way, it is consumed and reports zero pings.
        let mut b = buf(b"\r\nINVITE sip:bob SIP/2.0\r\n");
        assert_eq!(strip_leading_keepalives(&mut b), 0);
        assert_eq!(&b[..], b"INVITE sip:bob SIP/2.0\r\n");
    }

    #[test]
    fn double_crlf_is_one_ping() {
        let mut b = buf(b"\r\n\r\nINVITE sip:bob SIP/2.0\r\n");
        assert_eq!(strip_leading_keepalives(&mut b), 1);
        assert_eq!(&b[..], b"INVITE sip:bob SIP/2.0\r\n");
    }

    #[test]
    fn three_crlfs_is_one_ping_plus_pong() {
        // Three CRLFs = one ping (CRLFCRLF) + one pong (CRLF).
        let mut b = buf(b"\r\n\r\n\r\nINVITE sip:bob SIP/2.0\r\n");
        assert_eq!(strip_leading_keepalives(&mut b), 1);
        assert_eq!(&b[..], b"INVITE sip:bob SIP/2.0\r\n");
    }

    #[test]
    fn four_crlfs_is_two_pings() {
        let mut b = buf(b"\r\n\r\n\r\n\r\nINVITE sip:bob SIP/2.0\r\n");
        assert_eq!(strip_leading_keepalives(&mut b), 2);
        assert_eq!(&b[..], b"INVITE sip:bob SIP/2.0\r\n");
    }

    #[test]
    fn only_keepalives_drains_buffer() {
        let mut b = buf(b"\r\n\r\n");
        assert_eq!(strip_leading_keepalives(&mut b), 1);
        assert!(b.is_empty());
    }

    #[test]
    fn lone_cr_is_not_consumed() {
        // A bare CR without LF is not part of CRLF framing — leave it
        // in place. (In practice this would be a protocol error that
        // the SIP parser will surface.)
        let mut b = buf(b"\rJUNK");
        assert_eq!(strip_leading_keepalives(&mut b), 0);
        assert_eq!(&b[..], b"\rJUNK");
    }

    #[test]
    fn lf_alone_is_not_consumed() {
        let mut b = buf(b"\nJUNK");
        assert_eq!(strip_leading_keepalives(&mut b), 0);
        assert_eq!(&b[..], b"\nJUNK");
    }

    #[test]
    fn keepalive_interleaved_only_strips_leading() {
        // CRLFs *between* messages are framing within the message body
        // and must not be touched here. The helper only consumes the
        // leading run.
        let mut b = buf(b"\r\n\r\nINVITE sip:bob SIP/2.0\r\nVia: ...\r\n\r\n");
        assert_eq!(strip_leading_keepalives(&mut b), 1);
        assert_eq!(&b[..], b"INVITE sip:bob SIP/2.0\r\nVia: ...\r\n\r\n");
    }

    #[test]
    fn config_default_is_off() {
        let cfg = KeepAliveConfig::default();
        assert!(!cfg.send_pings);
        assert_eq!(cfg.ping_interval, DEFAULT_PING_INTERVAL);
    }

    #[test]
    fn config_enabled_helpers() {
        let cfg = KeepAliveConfig::enabled();
        assert!(cfg.send_pings);
        assert_eq!(cfg.ping_interval, DEFAULT_PING_INTERVAL);

        let cfg = KeepAliveConfig::enabled_with_interval(Duration::from_secs(30));
        assert!(cfg.send_pings);
        assert_eq!(cfg.ping_interval, Duration::from_secs(30));
    }
}
