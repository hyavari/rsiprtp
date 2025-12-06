//! Call transfer functionality (REFER method, RFC 3515).
//!
//! Implements both blind and attended call transfers.
//!
//! # Overview
//!
//! Call transfer uses the SIP REFER method:
//!
//! - **Blind Transfer**: Transfer without consulting the target first
//!   - A calls B, then REFERs B to C
//!   - B hangs up with A and calls C
//!
//! - **Attended Transfer**: Transfer after consulting the target (Replaces)
//!   - A calls B, puts B on hold
//!   - A calls C (consultation call)
//!   - A REFERs B to C with Replaces header
//!   - B replaces A's call to C
//!
//! # REFER Headers
//!
//! ```text
//! REFER sip:bob@example.com SIP/2.0
//! Refer-To: <sip:carol@example.com>
//! Referred-By: <sip:alice@example.com>
//! ```
//!
//! # NOTIFY for Transfer Progress
//!
//! The transferee sends NOTIFY messages to report progress:
//! ```text
//! NOTIFY sip:alice@example.com SIP/2.0
//! Event: refer
//! Subscription-State: active
//! Content-Type: message/sipfrag
//!
//! SIP/2.0 180 Ringing
//! ```

use std::collections::HashMap;
use thiserror::Error;

/// Transfer-related errors.
#[derive(Debug, Error)]
pub enum TransferError {
    /// Call not found.
    #[error("call not found: {0}")]
    CallNotFound(String),

    /// Invalid state for transfer.
    #[error("invalid state for transfer: {0}")]
    InvalidState(String),

    /// Transfer rejected.
    #[error("transfer rejected: {code}")]
    Rejected { code: u16 },

    /// Transfer failed.
    #[error("transfer failed: {0}")]
    Failed(String),

    /// Invalid Refer-To URI.
    #[error("invalid Refer-To URI: {0}")]
    InvalidReferTo(String),
}

/// Type of call transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferType {
    /// Blind transfer (no consultation).
    Blind,
    /// Attended transfer (with consultation call).
    Attended,
}

/// Transfer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    /// No transfer in progress.
    None,
    /// REFER sent, waiting for response.
    Pending,
    /// REFER accepted (202), waiting for progress.
    Accepted,
    /// Transfer in progress (got NOTIFY).
    InProgress,
    /// Transfer completed successfully.
    Completed,
    /// Transfer failed.
    Failed,
}

/// Transfer direction (who initiated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferRole {
    /// We are the transferor (initiated the transfer).
    Transferor,
    /// We are the transferee (being transferred).
    Transferee,
    /// We are the transfer target (receiving the transferred call).
    Target,
}

/// Refer-To header value.
#[derive(Debug, Clone)]
pub struct ReferTo {
    /// Target URI.
    pub uri: String,
    /// Optional Replaces header value for attended transfer.
    pub replaces: Option<ReplacesHeader>,
}

impl ReferTo {
    /// Create a simple Refer-To for blind transfer.
    pub fn blind(uri: &str) -> Self {
        Self {
            uri: uri.to_string(),
            replaces: None,
        }
    }

    /// Create a Refer-To with Replaces for attended transfer.
    pub fn attended(uri: &str, replaces: ReplacesHeader) -> Self {
        Self {
            uri: uri.to_string(),
            replaces: Some(replaces),
        }
    }

    /// Parse from header value.
    ///
    /// Format: `<sip:target@host>?Replaces=call-id%3Bto-tag%3Dx%3Bfrom-tag%3Dy`
    pub fn parse(s: &str) -> Result<Self, TransferError> {
        let s = s.trim();

        // Remove angle brackets if present
        let s = s.trim_start_matches('<').trim_end_matches('>');

        // Check for Replaces parameter
        if let Some((uri, params)) = s.split_once('?') {
            let replaces = parse_replaces_param(params)?;
            Ok(Self {
                uri: uri.to_string(),
                replaces,
            })
        } else {
            Ok(Self {
                uri: s.to_string(),
                replaces: None,
            })
        }
    }

    /// Format as header value.
    pub fn to_header(&self) -> String {
        if let Some(ref replaces) = self.replaces {
            format!("<{}?{}>", self.uri, replaces.to_uri_param())
        } else {
            format!("<{}>", self.uri)
        }
    }
}

/// Replaces header for attended transfer (RFC 3891).
#[derive(Debug, Clone)]
pub struct ReplacesHeader {
    /// Call-ID of the call to replace.
    pub call_id: String,
    /// To-tag of the call to replace.
    pub to_tag: String,
    /// From-tag of the call to replace.
    pub from_tag: String,
    /// Early-only flag.
    pub early_only: bool,
}

impl ReplacesHeader {
    /// Create a new Replaces header.
    pub fn new(call_id: &str, to_tag: &str, from_tag: &str) -> Self {
        Self {
            call_id: call_id.to_string(),
            to_tag: to_tag.to_string(),
            from_tag: from_tag.to_string(),
            early_only: false,
        }
    }

    /// Parse from Replaces header value.
    ///
    /// Format: `call-id;to-tag=x;from-tag=y`
    pub fn parse(s: &str) -> Result<Self, TransferError> {
        let mut parts = s.split(';');

        let call_id = parts.next()
            .ok_or_else(|| TransferError::InvalidReferTo("missing call-id".into()))?
            .trim()
            .to_string();

        let mut to_tag = None;
        let mut from_tag = None;
        let mut early_only = false;

        for part in parts {
            let part = part.trim();
            if let Some((key, value)) = part.split_once('=') {
                match key.to_lowercase().as_str() {
                    "to-tag" => to_tag = Some(value.to_string()),
                    "from-tag" => from_tag = Some(value.to_string()),
                    _ => {}
                }
            } else if part.eq_ignore_ascii_case("early-only") {
                early_only = true;
            }
        }

        Ok(Self {
            call_id,
            to_tag: to_tag.ok_or_else(|| TransferError::InvalidReferTo("missing to-tag".into()))?,
            from_tag: from_tag.ok_or_else(|| TransferError::InvalidReferTo("missing from-tag".into()))?,
            early_only,
        })
    }

    /// Format as header value.
    pub fn to_header(&self) -> String {
        let mut s = format!(
            "{};to-tag={};from-tag={}",
            self.call_id, self.to_tag, self.from_tag
        );
        if self.early_only {
            s.push_str(";early-only");
        }
        s
    }

    /// Format for URI parameter (URL-encoded).
    pub fn to_uri_param(&self) -> String {
        // URL encode semicolons and equals
        let encoded = format!(
            "Replaces={}%3Bto-tag%3D{}%3Bfrom-tag%3D{}",
            url_encode(&self.call_id),
            url_encode(&self.to_tag),
            url_encode(&self.from_tag)
        );
        if self.early_only {
            format!("{}%3Bearly-only", encoded)
        } else {
            encoded
        }
    }
}

/// Parse Replaces from URI parameter.
fn parse_replaces_param(params: &str) -> Result<Option<ReplacesHeader>, TransferError> {
    for param in params.split('&') {
        if let Some(value) = param.strip_prefix("Replaces=") {
            // URL decode
            let decoded = url_decode(value);
            return Ok(Some(ReplacesHeader::parse(&decoded)?));
        }
    }
    Ok(None)
}

/// Simple URL encoding for dialog identifiers.
fn url_encode(s: &str) -> String {
    s.replace('%', "%25")
        .replace(';', "%3B")
        .replace('=', "%3D")
        .replace('&', "%26")
        .replace('@', "%40")
}

/// Simple URL decoding.
fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Transfer progress notification.
#[derive(Debug, Clone)]
pub struct TransferProgress {
    /// SIP response code from the sipfrag.
    pub status_code: u16,
    /// Reason phrase.
    pub reason: String,
    /// Whether this is the final notification.
    pub final_: bool,
}

impl TransferProgress {
    /// Parse from NOTIFY body (message/sipfrag).
    pub fn parse_sipfrag(body: &str) -> Option<Self> {
        let first_line = body.lines().next()?;
        let parts: Vec<&str> = first_line.split_whitespace().collect();

        if parts.len() < 3 || parts[0] != "SIP/2.0" {
            return None;
        }

        let status_code: u16 = parts[1].parse().ok()?;
        let reason = parts[2..].join(" ");

        Some(Self {
            status_code,
            reason,
            final_: status_code >= 200,
        })
    }

    /// Check if transfer succeeded.
    pub fn is_success(&self) -> bool {
        self.status_code >= 200 && self.status_code < 300
    }

    /// Check if transfer is still in progress.
    pub fn is_provisional(&self) -> bool {
        self.status_code >= 100 && self.status_code < 200
    }
}

/// Transfer info for a call.
#[derive(Debug, Clone)]
pub struct TransferInfo {
    /// Transfer state.
    pub state: TransferState,
    /// Our role in the transfer.
    pub role: TransferRole,
    /// Transfer type.
    pub transfer_type: TransferType,
    /// Target URI (where we're transferring to).
    pub target: Option<String>,
    /// Call-ID of the REFER subscription.
    pub subscription_id: Option<String>,
    /// Last progress notification.
    pub last_progress: Option<TransferProgress>,
}

impl Default for TransferInfo {
    fn default() -> Self {
        Self {
            state: TransferState::None,
            role: TransferRole::Transferor,
            transfer_type: TransferType::Blind,
            target: None,
            subscription_id: None,
            last_progress: None,
        }
    }
}

/// Manages call transfers.
#[derive(Debug, Default)]
pub struct TransferManager {
    /// Transfer info per call.
    transfers: HashMap<String, TransferInfo>,
}

impl TransferManager {
    /// Create a new transfer manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Initiate a blind transfer.
    pub fn initiate_blind_transfer(
        &mut self,
        call_id: &str,
        target_uri: &str,
    ) -> Result<ReferTo, TransferError> {
        let info = self.transfers.entry(call_id.to_string()).or_default();

        if info.state != TransferState::None {
            return Err(TransferError::InvalidState("transfer already in progress".into()));
        }

        info.state = TransferState::Pending;
        info.role = TransferRole::Transferor;
        info.transfer_type = TransferType::Blind;
        info.target = Some(target_uri.to_string());

        Ok(ReferTo::blind(target_uri))
    }

    /// Initiate an attended transfer.
    pub fn initiate_attended_transfer(
        &mut self,
        call_id: &str,
        target_uri: &str,
        replaces: ReplacesHeader,
    ) -> Result<ReferTo, TransferError> {
        let info = self.transfers.entry(call_id.to_string()).or_default();

        if info.state != TransferState::None {
            return Err(TransferError::InvalidState("transfer already in progress".into()));
        }

        info.state = TransferState::Pending;
        info.role = TransferRole::Transferor;
        info.transfer_type = TransferType::Attended;
        info.target = Some(target_uri.to_string());

        Ok(ReferTo::attended(target_uri, replaces))
    }

    /// Handle REFER response.
    pub fn handle_refer_response(
        &mut self,
        call_id: &str,
        status_code: u16,
    ) -> Result<TransferState, TransferError> {
        let info = self.transfers.get_mut(call_id)
            .ok_or_else(|| TransferError::CallNotFound(call_id.to_string()))?;

        if (200..300).contains(&status_code) {
            info.state = TransferState::Accepted;
            Ok(TransferState::Accepted)
        } else if status_code >= 400 {
            info.state = TransferState::Failed;
            Err(TransferError::Rejected { code: status_code })
        } else {
            Ok(info.state)
        }
    }

    /// Handle incoming REFER (we are the transferee).
    pub fn handle_incoming_refer(
        &mut self,
        call_id: &str,
        refer_to: ReferTo,
    ) -> Result<&TransferInfo, TransferError> {
        let info = self.transfers.entry(call_id.to_string()).or_default();

        info.state = TransferState::InProgress;
        info.role = TransferRole::Transferee;
        info.transfer_type = if refer_to.replaces.is_some() {
            TransferType::Attended
        } else {
            TransferType::Blind
        };
        info.target = Some(refer_to.uri);

        Ok(info)
    }

    /// Handle NOTIFY with transfer progress.
    pub fn handle_notify(
        &mut self,
        call_id: &str,
        sipfrag: &str,
    ) -> Result<TransferProgress, TransferError> {
        let progress = TransferProgress::parse_sipfrag(sipfrag)
            .ok_or_else(|| TransferError::Failed("invalid sipfrag".into()))?;

        let info = self.transfers.get_mut(call_id)
            .ok_or_else(|| TransferError::CallNotFound(call_id.to_string()))?;

        if progress.is_success() {
            info.state = TransferState::Completed;
        } else if progress.final_ {
            info.state = TransferState::Failed;
        } else {
            info.state = TransferState::InProgress;
        }

        info.last_progress = Some(progress.clone());
        Ok(progress)
    }

    /// Get transfer state for a call.
    pub fn transfer_state(&self, call_id: &str) -> TransferState {
        self.transfers
            .get(call_id)
            .map(|i| i.state)
            .unwrap_or(TransferState::None)
    }

    /// Get transfer info for a call.
    pub fn transfer_info(&self, call_id: &str) -> Option<&TransferInfo> {
        self.transfers.get(call_id)
    }

    /// Clear transfer state for a call.
    pub fn clear(&mut self, call_id: &str) {
        self.transfers.remove(call_id);
    }
}

/// Build Refer-To header for blind transfer.
pub fn build_refer_to_blind(target_uri: &str) -> String {
    format!("Refer-To: <{}>", target_uri)
}

/// Build Refer-To header for attended transfer.
pub fn build_refer_to_attended(target_uri: &str, replaces: &ReplacesHeader) -> String {
    format!("Refer-To: <{}?{}>", target_uri, replaces.to_uri_param())
}

/// Build Referred-By header.
pub fn build_referred_by(transferor_uri: &str) -> String {
    format!("Referred-By: <{}>", transferor_uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_refer_to_blind() {
        let refer_to = ReferTo::blind("sip:carol@example.com");
        assert_eq!(refer_to.to_header(), "<sip:carol@example.com>");
        assert!(refer_to.replaces.is_none());
    }

    #[test]
    fn test_refer_to_parse_blind() {
        let refer_to = ReferTo::parse("<sip:carol@example.com>").unwrap();
        assert_eq!(refer_to.uri, "sip:carol@example.com");
        assert!(refer_to.replaces.is_none());
    }

    #[test]
    fn test_replaces_header() {
        let replaces = ReplacesHeader::new("abc123", "to1", "from1");
        let header = replaces.to_header();
        assert!(header.contains("abc123"));
        assert!(header.contains("to-tag=to1"));
        assert!(header.contains("from-tag=from1"));

        let parsed = ReplacesHeader::parse(&header).unwrap();
        assert_eq!(parsed.call_id, "abc123");
        assert_eq!(parsed.to_tag, "to1");
        assert_eq!(parsed.from_tag, "from1");
    }

    #[test]
    fn test_replaces_uri_encoding() {
        let replaces = ReplacesHeader::new("abc;123", "to=1", "from&1");
        let param = replaces.to_uri_param();

        // Should be URL encoded
        assert!(param.contains("abc%3B123"));
        assert!(param.contains("to%3D1"));
        assert!(param.contains("from%261"));
    }

    #[test]
    fn test_transfer_progress() {
        let sipfrag = "SIP/2.0 180 Ringing\r\n";
        let progress = TransferProgress::parse_sipfrag(sipfrag).unwrap();
        assert_eq!(progress.status_code, 180);
        assert_eq!(progress.reason, "Ringing");
        assert!(!progress.final_);
        assert!(progress.is_provisional());

        let sipfrag = "SIP/2.0 200 OK\r\n";
        let progress = TransferProgress::parse_sipfrag(sipfrag).unwrap();
        assert_eq!(progress.status_code, 200);
        assert!(progress.final_);
        assert!(progress.is_success());
    }

    #[test]
    fn test_transfer_manager_blind() {
        let mut manager = TransferManager::new();

        let refer_to = manager
            .initiate_blind_transfer("call-1", "sip:carol@example.com")
            .unwrap();

        assert_eq!(manager.transfer_state("call-1"), TransferState::Pending);
        assert!(refer_to.replaces.is_none());

        // REFER accepted
        manager.handle_refer_response("call-1", 202).unwrap();
        assert_eq!(manager.transfer_state("call-1"), TransferState::Accepted);

        // Progress notification
        manager.handle_notify("call-1", "SIP/2.0 180 Ringing\r\n").unwrap();
        assert_eq!(manager.transfer_state("call-1"), TransferState::InProgress);

        // Success
        manager.handle_notify("call-1", "SIP/2.0 200 OK\r\n").unwrap();
        assert_eq!(manager.transfer_state("call-1"), TransferState::Completed);
    }

    #[test]
    fn test_transfer_manager_attended() {
        let mut manager = TransferManager::new();

        let replaces = ReplacesHeader::new("consultation-call", "tag1", "tag2");
        let refer_to = manager
            .initiate_attended_transfer("call-1", "sip:carol@example.com", replaces)
            .unwrap();

        assert!(refer_to.replaces.is_some());
        assert_eq!(
            manager.transfer_info("call-1").unwrap().transfer_type,
            TransferType::Attended
        );
    }

    #[test]
    fn test_incoming_refer() {
        let mut manager = TransferManager::new();

        let refer_to = ReferTo::blind("sip:carol@example.com");
        let info = manager.handle_incoming_refer("call-1", refer_to).unwrap();

        assert_eq!(info.role, TransferRole::Transferee);
        assert_eq!(info.state, TransferState::InProgress);
    }
}
