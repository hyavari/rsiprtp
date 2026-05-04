//! Typed `Contact` header (RFC 3261 §20.10).
//!
//! Two wire forms (per RFC 3261 §20.10 ABNF):
//!
//! ```text
//! Contact   =  ("Contact" / "m") HCOLON
//!              ( STAR / (contact-param *(COMMA contact-param)) )
//! contact-param  =  (name-addr / addr-spec) *(SEMI contact-params)
//! ```
//!
//! The `STAR` form (`Contact: *`) is only valid in REGISTER
//! requests with `Expires: 0` to clear all bindings (§10.3).
//! rsip's `typed::Contact` does NOT model the wildcard — it
//! delegates that to the untyped form. Our parser handles the
//! wildcard explicitly so callers don't have to special-case
//! the literal `*` string.
//!
//! Field shape of the [`Contact::Addr`] variant mirrors
//! `rsip::typed::Contact` (display_name + uri + params), so the
//! Tier-2 cutover in M8 is a near-drop-in replacement.

use super::super::name_addr::NameAddr;
use crate::core::SipError;
use crate::sip::uri::SipUri;
use std::fmt;

/// Internal: parse `;k=v;k2;k3=v3` (the trailing parameter chain
/// after a `Contact: *`) into a parameter list. Mirrors
/// `name_addr::parse_params` but is deliberately minimal — wildcard
/// params do not carry quoted-string values per typical use
/// (`*;expires=0`), and the strict subset keeps this private.
fn parse_wildcard_params(s: &str) -> Result<Vec<(String, Option<String>)>, SipError> {
    let s = s.trim_start();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    if !s.starts_with(';') {
        return Err(SipError::InvalidHeader(format!(
            "trailing data after wildcard Contact (expected ';' or end): {s:?}",
        )));
    }
    let mut params = Vec::new();
    for part in s[1..].split(';') {
        let chunk = part.trim();
        if chunk.is_empty() {
            continue;
        }
        if let Some(eq) = chunk.find('=') {
            let (k, v) = chunk.split_at(eq);
            params.push((k.trim().to_string(), Some(v[1..].trim().to_string())));
        } else {
            params.push((chunk.to_string(), None));
        }
    }
    Ok(params)
}

/// Typed form of one `Contact` header value.
///
/// One `Header::Contact(value)` corresponds to one parsed
/// `Contact`. We do not split a comma-separated value at this
/// level — that is the caller's concern (matches our Tier-1
/// framing, which preserves the value verbatim).
#[derive(Debug, Clone, PartialEq)]
pub enum Contact {
    /// `Contact: *` (optionally with parameters such as
    /// `*;expires=0`) — REGISTER wildcard (RFC 3261 §10.2.2 +
    /// §10.3). The wildcard form clears registrations; in the
    /// common unbinding pattern an `expires=0` parameter is
    /// attached directly to the `*` rather than coming from a
    /// separate `Expires:` header. Semantic interpretation is
    /// the registrar's responsibility, not the parser's.
    Wildcard {
        /// Trailing parameters after `*` (e.g. `expires=0`).
        /// Empty for the bare-`*` form.
        params: Vec<(String, Option<String>)>,
    },
    /// Normal name-address form: optional display name, URI,
    /// and trailing `;param=value` chain.
    Addr(ContactAddr),
}

/// Inner data for a non-wildcard `Contact`.
///
/// Field shape mirrors `rsip::typed::Contact`. Use
/// [`Contact::expires`] / [`Contact::q_value`] for typed
/// accessors that work on either variant.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactAddr {
    /// Display name (with quoted-pair escapes resolved and
    /// surrounding quotes removed; see [`NameAddr`]).
    pub display_name: Option<String>,
    /// Contact URI.
    pub uri: SipUri,
    /// Header parameters in wire order.
    pub params: Vec<(String, Option<String>)>,
}

impl Contact {
    /// True if this is the wildcard form (`Contact: *` with or
    /// without trailing parameters).
    pub fn is_wildcard(&self) -> bool {
        matches!(self, Contact::Wildcard { .. })
    }

    /// Parse a `Contact` header value (the part after
    /// `Contact: `).
    ///
    /// Detects the wildcard form (`*` optionally followed by
    /// `;param=value...` per RFC 3261 §10.2.2 — the common
    /// REGISTER unbinding shape `*;expires=0`) first. Otherwise
    /// delegates to [`NameAddr`] and wraps in [`Contact::Addr`].
    pub fn parse(value: &str) -> Result<Contact, SipError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(SipError::InvalidHeader(
                "empty Contact header value".to_string(),
            ));
        }
        // Wildcard: `*` or `*;param=value...`. We accept any
        // amount of LWS between the `*` and the `;`.
        if let Some(rest) = trimmed.strip_prefix('*') {
            let rest_trimmed = rest.trim_start();
            let params = parse_wildcard_params(rest_trimmed)?;
            return Ok(Contact::Wildcard { params });
        }
        let na = NameAddr::parse(trimmed)?;
        Ok(Contact::Addr(ContactAddr {
            display_name: na.display_name,
            uri: na.uri,
            params: na.parameters,
        }))
    }

    /// Lookup the `expires` parameter (RFC 3261 §20.10 / §10.2.2)
    /// — the preferred lifetime in seconds for this binding.
    /// Returns `None` for an absent param or a non-numeric value.
    /// Now reads from wildcard params too (`*;expires=0`).
    pub fn expires(&self) -> Option<u32> {
        let params = match self {
            Contact::Wildcard { params } => params,
            Contact::Addr(a) => &a.params,
        };
        params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("expires"))
            .and_then(|(_, v)| v.as_deref())
            .and_then(|v| v.parse::<u32>().ok())
    }

    /// Lookup the `q` parameter (RFC 3261 §20.10) — the
    /// "quality" / preference for this binding, in `[0.0,
    /// 1.0]`. Returns `None` for an absent param or a
    /// non-numeric value. We do NOT range-check; the registrar
    /// can. Reads from wildcard params too for symmetry, though
    /// `q=` on `*` is not a standard pattern.
    pub fn q_value(&self) -> Option<f32> {
        let params = match self {
            Contact::Wildcard { params } => params,
            Contact::Addr(a) => &a.params,
        };
        params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("q"))
            .and_then(|(_, v)| v.as_deref())
            .and_then(|v| v.parse::<f32>().ok())
    }
}

impl fmt::Display for Contact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Contact::Wildcard { params } => {
                f.write_str("*")?;
                for (k, v) in params {
                    match v {
                        Some(v) => write!(f, ";{}={}", k, v)?,
                        None => write!(f, ";{}", k)?,
                    }
                }
                Ok(())
            }
            Contact::Addr(a) => {
                // Round-trip through NameAddr's Display so
                // display-name quoting logic stays in one place.
                let na = NameAddr {
                    display_name: a.display_name.clone(),
                    uri: a.uri.clone(),
                    parameters: a.params.clone(),
                };
                write!(f, "{}", na)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_bracketed() {
        let c = Contact::parse("<sip:alice@192.168.1.1:5060>").unwrap();
        match c {
            Contact::Addr(a) => {
                assert_eq!(a.display_name, None);
                assert_eq!(a.uri.user(), Some("alice"));
                assert!(a.params.is_empty());
            }
            Contact::Wildcard { .. } => panic!("expected Addr, got Wildcard"),
        }
    }

    #[test]
    fn test_parse_with_display_name() {
        let c = Contact::parse(r#""Alice" <sip:alice@example.com>;expires=3600"#).unwrap();
        assert!(!c.is_wildcard());
        assert_eq!(c.expires(), Some(3600));
    }

    #[test]
    fn test_parse_token_display_name() {
        let c = Contact::parse("Bob <sip:bob@example.com>").unwrap();
        if let Contact::Addr(a) = c {
            assert_eq!(a.display_name, Some("Bob".to_string()));
        } else {
            panic!("expected Addr");
        }
    }

    #[test]
    fn test_parse_bare_addr_spec() {
        let c = Contact::parse("sip:alice@example.com").unwrap();
        if let Contact::Addr(a) = c {
            assert_eq!(a.display_name, None);
            assert_eq!(a.uri.user(), Some("alice"));
        } else {
            panic!("expected Addr");
        }
    }

    #[test]
    fn test_parse_wildcard() {
        let c = Contact::parse("*").unwrap();
        assert!(c.is_wildcard());
        assert_eq!(c.expires(), None);
        assert_eq!(c.q_value(), None);
    }

    #[test]
    fn test_parse_wildcard_trims_whitespace() {
        let c = Contact::parse("  *  ").unwrap();
        assert!(c.is_wildcard());
    }

    /// Bare `*` parses to `Wildcard { params: [] }`. M5 backlog
    /// item — RFC 3261 §10.2.2 wildcard-with-params support.
    #[test]
    fn test_wildcard_simple() {
        let c = Contact::parse("*").unwrap();
        match c {
            Contact::Wildcard { params } => {
                assert!(params.is_empty(), "bare * has no params");
            }
            Contact::Addr(_) => panic!("expected Wildcard"),
        }
    }

    /// RFC 3261 §10.2.2: `*;expires=0` is the canonical wildcard
    /// unbinding shape — the wildcard variant carries the
    /// `expires=0` directly. Both `is_wildcard()` and `expires()`
    /// must work.
    #[test]
    fn test_wildcard_with_expires_zero() {
        let c = Contact::parse("*;expires=0").unwrap();
        assert!(c.is_wildcard());
        assert_eq!(c.expires(), Some(0));
    }

    /// Wildcard with multiple parameters round-trips through
    /// Display.
    #[test]
    fn test_wildcard_with_multiple_params() {
        let v = "*;expires=0;custom=foo";
        let c = Contact::parse(v).unwrap();
        assert!(c.is_wildcard());
        assert_eq!(c.expires(), Some(0));
        // Params order preserved on Display.
        assert_eq!(c.to_string(), v);
        // Reparse for a structural round-trip.
        let reparsed = Contact::parse(&c.to_string()).unwrap();
        assert_eq!(c, reparsed);
    }

    #[test]
    fn test_parse_expires_param() {
        let c = Contact::parse("<sip:a@b>;expires=7200").unwrap();
        assert_eq!(c.expires(), Some(7200));
    }

    #[test]
    fn test_parse_expires_zero() {
        // Expires=0 with wildcard is the deregister-all form;
        // here just on a normal addr we still read it as 0.
        let c = Contact::parse("<sip:a@b>;expires=0").unwrap();
        assert_eq!(c.expires(), Some(0));
    }

    #[test]
    fn test_parse_q_value() {
        let c = Contact::parse("<sip:a@b>;q=0.5").unwrap();
        assert_eq!(c.q_value(), Some(0.5));
    }

    #[test]
    fn test_parse_q_value_one_decimal() {
        let c = Contact::parse("<sip:a@b>;q=0.7").unwrap();
        assert_eq!(c.q_value(), Some(0.7));
    }

    #[test]
    fn test_parse_q_value_one_point_zero() {
        let c = Contact::parse("<sip:a@b>;q=1.0").unwrap();
        assert_eq!(c.q_value(), Some(1.0));
    }

    #[test]
    fn test_parse_all_params() {
        let c = Contact::parse(r#""Bob" <sip:bob@example.com>;expires=7200;q=0.8"#).unwrap();
        assert_eq!(c.expires(), Some(7200));
        assert_eq!(c.q_value(), Some(0.8));
    }

    #[test]
    fn test_parse_param_lookup_case_insensitive() {
        let c = Contact::parse("<sip:a@b>;EXPIRES=300;Q=0.4").unwrap();
        assert_eq!(c.expires(), Some(300));
        assert_eq!(c.q_value(), Some(0.4));
    }

    #[test]
    fn test_parse_expires_invalid_returns_none() {
        let c = Contact::parse("<sip:a@b>;expires=abc").unwrap();
        // Param is preserved on the raw struct; typed accessor
        // returns None.
        assert_eq!(c.expires(), None);
    }

    #[test]
    fn test_parse_invalid_rejected() {
        assert!(Contact::parse("").is_err());
        assert!(Contact::parse("not a contact <").is_err());
    }

    #[test]
    fn test_display_round_trip_simple() {
        let v = "<sip:alice@example.com>;expires=3600";
        let c = Contact::parse(v).unwrap();
        assert_eq!(c.to_string(), v);
    }

    #[test]
    fn test_display_round_trip_token_name() {
        let v = "Bob <sip:bob@example.com>;expires=300";
        let c = Contact::parse(v).unwrap();
        assert_eq!(c.to_string(), v);
    }

    #[test]
    fn test_display_round_trip_quoted_name() {
        let v = r#""Alice Smith" <sip:a@b>;expires=60"#;
        let c = Contact::parse(v).unwrap();
        assert_eq!(c.to_string(), v);
    }

    #[test]
    fn test_display_round_trip_wildcard() {
        let c = Contact::parse("*").unwrap();
        assert_eq!(c.to_string(), "*");
    }
}
