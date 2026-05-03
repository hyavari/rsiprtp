//! Typed `CSeq` header (RFC 3261 §20.16).
//!
//! `CSeq: <seq> <Method>`
//!
//! Two whitespace-separated tokens. Per RFC 3261 §8.1.1.5 the
//! sequence number is a 32-bit unsigned integer (the spec
//! limits it to `2**31 - 1` initial values; we accept the full
//! `u32` range — overflow is the transaction layer's concern,
//! not the parser's).

use super::super::method::Method;
use crate::core::SipError;
use std::fmt;
use std::str::FromStr;

/// Typed form of the `CSeq` header.
///
/// Field shape mirrors `rsip::typed::CSeq`: public `seq` and
/// `method` so call sites can field-access without going
/// through accessors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CSeq {
    /// Sequence number. RFC 3261 §8.1.1.5 limits initial values
    /// to `2**31 - 1`; we accept the full `u32` range.
    pub seq: u32,
    /// Method of the request this CSeq corresponds to.
    pub method: Method,
}

impl CSeq {
    /// Parse a `CSeq` header value (the part after `CSeq: `).
    ///
    /// Splits on ASCII whitespace; expects exactly two tokens.
    /// The first is parsed as `u32`, the second as `Method`
    /// (case-insensitive — see [`Method::from_str`]).
    pub fn parse(value: &str) -> Result<CSeq, SipError> {
        let mut parts = value.split_whitespace();
        let seq_tok = parts.next().ok_or_else(|| {
            SipError::InvalidHeader(format!("CSeq missing sequence number: {value:?}",))
        })?;
        let method_tok = parts
            .next()
            .ok_or_else(|| SipError::InvalidHeader(format!("CSeq missing method: {value:?}")))?;
        if parts.next().is_some() {
            return Err(SipError::InvalidHeader(format!(
                "CSeq has trailing tokens after method: {value:?}",
            )));
        }
        let seq: u32 = seq_tok.parse().map_err(|_| {
            SipError::InvalidHeader(format!("CSeq sequence number is not a u32: {seq_tok:?}",))
        })?;
        let method = Method::from_str(method_tok)
            .map_err(|e| SipError::InvalidHeader(format!("CSeq method invalid: {e}")))?;
        Ok(CSeq { seq, method })
    }
}

impl fmt::Display for CSeq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.seq, self.method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_invite() {
        let c = CSeq::parse("1 INVITE").unwrap();
        assert_eq!(c.seq, 1);
        assert_eq!(c.method, Method::Invite);
    }

    #[test]
    fn test_parse_bye() {
        let c = CSeq::parse("42 BYE").unwrap();
        assert_eq!(c.seq, 42);
        assert_eq!(c.method, Method::Bye);
    }

    #[test]
    fn test_parse_extra_whitespace() {
        // Multiple internal spaces, leading and trailing whitespace.
        let c = CSeq::parse("  314159   INVITE  ").unwrap();
        assert_eq!(c.seq, 314159);
        assert_eq!(c.method, Method::Invite);
    }

    #[test]
    fn test_parse_method_case_insensitive() {
        // Method::from_str is case-insensitive (matches rsip).
        let c = CSeq::parse("1 invite").unwrap();
        assert_eq!(c.method, Method::Invite);
        let c = CSeq::parse("1 Subscribe").unwrap();
        assert_eq!(c.method, Method::Subscribe);
    }

    #[test]
    fn test_parse_max_u32() {
        let c = CSeq::parse(&format!("{} INVITE", u32::MAX)).unwrap();
        assert_eq!(c.seq, u32::MAX);
    }

    #[test]
    fn test_parse_zero() {
        // RFC 3261 says initial CSeq must be < 2**31, but doesn't
        // forbid zero at the grammar level. Accept it.
        let c = CSeq::parse("0 OPTIONS").unwrap();
        assert_eq!(c.seq, 0);
    }

    #[test]
    fn test_parse_single_token_rejected() {
        assert!(CSeq::parse("1").is_err());
        assert!(CSeq::parse("INVITE").is_err());
    }

    #[test]
    fn test_parse_three_tokens_rejected() {
        assert!(CSeq::parse("1 INVITE extra").is_err());
    }

    #[test]
    fn test_parse_non_numeric_seq_rejected() {
        assert!(CSeq::parse("abc INVITE").is_err());
    }

    #[test]
    fn test_parse_negative_seq_rejected() {
        // u32 doesn't accept negative; -1 fails to parse.
        assert!(CSeq::parse("-1 INVITE").is_err());
    }

    #[test]
    fn test_parse_overflow_seq_rejected() {
        // u32::MAX + 1 doesn't fit.
        assert!(CSeq::parse("4294967296 INVITE").is_err());
    }

    #[test]
    fn test_parse_unknown_method_rejected() {
        assert!(CSeq::parse("1 BOGUS").is_err());
    }

    #[test]
    fn test_parse_empty_rejected() {
        assert!(CSeq::parse("").is_err());
        assert!(CSeq::parse("   ").is_err());
    }

    #[test]
    fn test_display_round_trip() {
        for (input, expected) in [
            ("1 INVITE", "1 INVITE"),
            ("42 BYE", "42 BYE"),
            ("314159 INVITE", "314159 INVITE"),
            // Display canonicalizes method to upper-case.
            ("1 invite", "1 INVITE"),
        ] {
            let c = CSeq::parse(input).unwrap();
            assert_eq!(c.to_string(), expected);
        }
    }
}
