//! Call hold and resume functionality.
//!
//! Implements SIP call hold using re-INVITE with SDP direction attributes.
//!
//! # Overview
//!
//! Call hold is implemented by sending a re-INVITE with modified SDP:
//! - `a=sendonly` - Local hold (we stop sending, remote can still send)
//! - `a=recvonly` - Remote hold (we receive only)
//! - `a=inactive` - Full hold (no media in either direction)
//!
//! Music on hold (MOH) can be provided by continuing to send audio
//! while in `sendonly` mode.
//!
//! # Example
//!
//! ```rust,ignore
//! use mdsiprtp_session::{HoldManager, HoldState, HoldRequest};
//!
//! // Put call on hold
//! let reinvite = hold_manager.create_hold_request(call_id)?;
//! send_reinvite(reinvite);
//!
//! // Resume from hold
//! let reinvite = hold_manager.create_resume_request(call_id)?;
//! send_reinvite(reinvite);
//! ```

use std::collections::HashMap;
use thiserror::Error;

/// Hold-related errors.
#[derive(Debug, Error)]
pub enum HoldError {
    /// Call not found.
    #[error("call not found: {0}")]
    CallNotFound(String),

    /// Invalid state for operation.
    #[error("invalid state for hold operation: {0}")]
    InvalidState(String),

    /// SDP generation failed.
    #[error("failed to generate hold SDP: {0}")]
    SdpError(String),
}

/// Media direction for hold states (RFC 3264).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaDirection {
    /// Send and receive (normal call).
    SendRecv,
    /// Send only (local hold with optional MOH).
    SendOnly,
    /// Receive only (waiting for remote media).
    RecvOnly,
    /// Inactive (full hold, no media).
    Inactive,
}

impl MediaDirection {
    /// Parse from SDP attribute value.
    pub fn from_sdp(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sendrecv" => Some(MediaDirection::SendRecv),
            "sendonly" => Some(MediaDirection::SendOnly),
            "recvonly" => Some(MediaDirection::RecvOnly),
            "inactive" => Some(MediaDirection::Inactive),
            _ => None,
        }
    }

    /// Convert to SDP attribute value.
    pub fn to_sdp(&self) -> &'static str {
        match self {
            MediaDirection::SendRecv => "sendrecv",
            MediaDirection::SendOnly => "sendonly",
            MediaDirection::RecvOnly => "recvonly",
            MediaDirection::Inactive => "inactive",
        }
    }

    /// Check if this direction allows sending media.
    pub fn can_send(&self) -> bool {
        matches!(self, MediaDirection::SendRecv | MediaDirection::SendOnly)
    }

    /// Check if this direction allows receiving media.
    pub fn can_recv(&self) -> bool {
        matches!(self, MediaDirection::SendRecv | MediaDirection::RecvOnly)
    }

    /// Get the expected direction for the remote side.
    pub fn remote_direction(&self) -> MediaDirection {
        match self {
            MediaDirection::SendRecv => MediaDirection::SendRecv,
            MediaDirection::SendOnly => MediaDirection::RecvOnly,
            MediaDirection::RecvOnly => MediaDirection::SendOnly,
            MediaDirection::Inactive => MediaDirection::Inactive,
        }
    }
}

/// Hold state for a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldState {
    /// Call is active (not on hold).
    Active,
    /// We placed the call on hold (local hold).
    LocalHold,
    /// Remote party placed us on hold.
    RemoteHold,
    /// Both parties on hold.
    BothHold,
}

impl HoldState {
    /// Check if we are holding the remote.
    pub fn is_local_hold(&self) -> bool {
        matches!(self, HoldState::LocalHold | HoldState::BothHold)
    }

    /// Check if remote is holding us.
    pub fn is_remote_hold(&self) -> bool {
        matches!(self, HoldState::RemoteHold | HoldState::BothHold)
    }

    /// Check if call is active (not on hold by anyone).
    pub fn is_active(&self) -> bool {
        *self == HoldState::Active
    }
}

/// Request to change hold state.
#[derive(Debug, Clone)]
pub struct HoldRequest {
    /// Call ID.
    pub call_id: String,
    /// Requested media direction.
    pub direction: MediaDirection,
    /// Whether to provide music on hold.
    pub music_on_hold: bool,
}

/// Response to hold request.
#[derive(Debug, Clone)]
pub struct HoldResponse {
    /// Call ID.
    pub call_id: String,
    /// Resulting media direction.
    pub direction: MediaDirection,
    /// New hold state.
    pub state: HoldState,
}

/// Call hold state tracking.
#[derive(Debug, Clone)]
pub struct CallHoldInfo {
    /// Current hold state.
    pub state: HoldState,
    /// Current local direction.
    pub local_direction: MediaDirection,
    /// Current remote direction.
    pub remote_direction: MediaDirection,
    /// Pending hold request.
    pub pending_request: Option<HoldRequest>,
}

impl Default for CallHoldInfo {
    fn default() -> Self {
        Self {
            state: HoldState::Active,
            local_direction: MediaDirection::SendRecv,
            remote_direction: MediaDirection::SendRecv,
            pending_request: None,
        }
    }
}

/// Manages hold state for calls.
#[derive(Debug, Default)]
pub struct HoldManager {
    /// Hold info per call.
    calls: HashMap<String, CallHoldInfo>,
}

impl HoldManager {
    /// Create a new hold manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new call.
    pub fn add_call(&mut self, call_id: &str) {
        self.calls.insert(call_id.to_string(), CallHoldInfo::default());
    }

    /// Remove a call.
    pub fn remove_call(&mut self, call_id: &str) {
        self.calls.remove(call_id);
    }

    /// Get hold state for a call.
    pub fn hold_state(&self, call_id: &str) -> Option<HoldState> {
        self.calls.get(call_id).map(|info| info.state)
    }

    /// Get call hold info.
    pub fn call_info(&self, call_id: &str) -> Option<&CallHoldInfo> {
        self.calls.get(call_id)
    }

    /// Create a hold request (put call on hold).
    ///
    /// Returns the SDP direction to use in re-INVITE.
    pub fn create_hold_request(
        &mut self,
        call_id: &str,
        inactive: bool,
    ) -> Result<MediaDirection, HoldError> {
        let info = self.calls.get_mut(call_id)
            .ok_or_else(|| HoldError::CallNotFound(call_id.to_string()))?;

        if info.state == HoldState::LocalHold || info.state == HoldState::BothHold {
            return Err(HoldError::InvalidState("already on local hold".into()));
        }

        let direction = if inactive {
            MediaDirection::Inactive
        } else {
            MediaDirection::SendOnly
        };

        info.pending_request = Some(HoldRequest {
            call_id: call_id.to_string(),
            direction,
            music_on_hold: !inactive,
        });

        Ok(direction)
    }

    /// Create a resume request (take call off hold).
    ///
    /// Returns the SDP direction to use in re-INVITE.
    pub fn create_resume_request(&mut self, call_id: &str) -> Result<MediaDirection, HoldError> {
        let info = self.calls.get_mut(call_id)
            .ok_or_else(|| HoldError::CallNotFound(call_id.to_string()))?;

        if !info.state.is_local_hold() {
            return Err(HoldError::InvalidState("not on local hold".into()));
        }

        let direction = MediaDirection::SendRecv;

        info.pending_request = Some(HoldRequest {
            call_id: call_id.to_string(),
            direction,
            music_on_hold: false,
        });

        Ok(direction)
    }

    /// Handle successful re-INVITE response for hold/resume.
    pub fn handle_hold_response(
        &mut self,
        call_id: &str,
        remote_direction: MediaDirection,
    ) -> Result<HoldResponse, HoldError> {
        let info = self.calls.get_mut(call_id)
            .ok_or_else(|| HoldError::CallNotFound(call_id.to_string()))?;

        let request = info.pending_request.take()
            .ok_or_else(|| HoldError::InvalidState("no pending hold request".into()))?;

        info.local_direction = request.direction;
        info.remote_direction = remote_direction;

        // Update hold state based on directions
        info.state = compute_hold_state(info.local_direction, info.remote_direction);

        Ok(HoldResponse {
            call_id: call_id.to_string(),
            direction: info.local_direction,
            state: info.state,
        })
    }

    /// Handle incoming re-INVITE with changed direction (remote hold).
    pub fn handle_remote_hold(
        &mut self,
        call_id: &str,
        remote_direction: MediaDirection,
    ) -> Result<HoldState, HoldError> {
        let info = self.calls.get_mut(call_id)
            .ok_or_else(|| HoldError::CallNotFound(call_id.to_string()))?;

        info.remote_direction = remote_direction;

        // Our direction should be the inverse
        info.local_direction = remote_direction.remote_direction();

        // Update hold state
        info.state = compute_hold_state(info.local_direction, info.remote_direction);

        Ok(info.state)
    }

    /// Cancel pending hold request.
    pub fn cancel_pending(&mut self, call_id: &str) {
        if let Some(info) = self.calls.get_mut(call_id) {
            info.pending_request = None;
        }
    }

    /// Check if we can send media for a call.
    pub fn can_send(&self, call_id: &str) -> bool {
        self.calls
            .get(call_id)
            .map(|info| info.local_direction.can_send())
            .unwrap_or(false)
    }

    /// Check if we can receive media for a call.
    pub fn can_recv(&self, call_id: &str) -> bool {
        self.calls
            .get(call_id)
            .map(|info| info.local_direction.can_recv())
            .unwrap_or(false)
    }
}

/// Compute hold state from local and remote directions.
fn compute_hold_state(local: MediaDirection, remote: MediaDirection) -> HoldState {
    let local_holding = matches!(local, MediaDirection::SendOnly | MediaDirection::Inactive);
    let remote_holding = matches!(remote, MediaDirection::SendOnly | MediaDirection::Inactive);

    match (local_holding, remote_holding) {
        (false, false) => HoldState::Active,
        (true, false) => HoldState::LocalHold,
        (false, true) => HoldState::RemoteHold,
        (true, true) => HoldState::BothHold,
    }
}

/// Create SDP attribute line for direction.
pub fn sdp_direction_attribute(direction: MediaDirection) -> String {
    format!("a={}", direction.to_sdp())
}

/// Parse direction from SDP media or session level.
pub fn parse_sdp_direction(sdp: &str) -> MediaDirection {
    for line in sdp.lines() {
        let line = line.trim();
        if let Some(attr) = line.strip_prefix("a=") {
            if let Some(dir) = MediaDirection::from_sdp(attr) {
                return dir;
            }
        }
    }
    // Default is sendrecv if not specified
    MediaDirection::SendRecv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_direction() {
        assert_eq!(MediaDirection::from_sdp("sendrecv"), Some(MediaDirection::SendRecv));
        assert_eq!(MediaDirection::from_sdp("SENDONLY"), Some(MediaDirection::SendOnly));
        assert_eq!(MediaDirection::from_sdp("recvonly"), Some(MediaDirection::RecvOnly));
        assert_eq!(MediaDirection::from_sdp("inactive"), Some(MediaDirection::Inactive));

        assert!(MediaDirection::SendRecv.can_send());
        assert!(MediaDirection::SendRecv.can_recv());
        assert!(MediaDirection::SendOnly.can_send());
        assert!(!MediaDirection::SendOnly.can_recv());
        assert!(!MediaDirection::Inactive.can_send());
        assert!(!MediaDirection::Inactive.can_recv());
    }

    #[test]
    fn test_remote_direction() {
        assert_eq!(
            MediaDirection::SendOnly.remote_direction(),
            MediaDirection::RecvOnly
        );
        assert_eq!(
            MediaDirection::RecvOnly.remote_direction(),
            MediaDirection::SendOnly
        );
        assert_eq!(
            MediaDirection::SendRecv.remote_direction(),
            MediaDirection::SendRecv
        );
    }

    #[test]
    fn test_hold_state() {
        assert!(HoldState::Active.is_active());
        assert!(!HoldState::Active.is_local_hold());
        assert!(!HoldState::Active.is_remote_hold());

        assert!(HoldState::LocalHold.is_local_hold());
        assert!(!HoldState::LocalHold.is_remote_hold());

        assert!(!HoldState::RemoteHold.is_local_hold());
        assert!(HoldState::RemoteHold.is_remote_hold());

        assert!(HoldState::BothHold.is_local_hold());
        assert!(HoldState::BothHold.is_remote_hold());
    }

    #[test]
    fn test_hold_manager_lifecycle() {
        let mut manager = HoldManager::new();

        manager.add_call("call-1");
        assert_eq!(manager.hold_state("call-1"), Some(HoldState::Active));

        // Put on hold
        let direction = manager.create_hold_request("call-1", false).unwrap();
        assert_eq!(direction, MediaDirection::SendOnly);

        // Simulate response
        let response = manager
            .handle_hold_response("call-1", MediaDirection::RecvOnly)
            .unwrap();
        assert_eq!(response.state, HoldState::LocalHold);

        // Resume
        let direction = manager.create_resume_request("call-1").unwrap();
        assert_eq!(direction, MediaDirection::SendRecv);

        let response = manager
            .handle_hold_response("call-1", MediaDirection::SendRecv)
            .unwrap();
        assert_eq!(response.state, HoldState::Active);

        manager.remove_call("call-1");
        assert_eq!(manager.hold_state("call-1"), None);
    }

    #[test]
    fn test_remote_hold() {
        let mut manager = HoldManager::new();
        manager.add_call("call-1");

        // Remote puts us on hold
        let state = manager
            .handle_remote_hold("call-1", MediaDirection::SendOnly)
            .unwrap();
        assert_eq!(state, HoldState::RemoteHold);

        // Remote resumes
        let state = manager
            .handle_remote_hold("call-1", MediaDirection::SendRecv)
            .unwrap();
        assert_eq!(state, HoldState::Active);
    }

    #[test]
    fn test_parse_sdp_direction() {
        let sdp = "v=0\r\nm=audio 5000 RTP/AVP 0\r\na=sendonly\r\n";
        assert_eq!(parse_sdp_direction(sdp), MediaDirection::SendOnly);

        let sdp = "v=0\r\nm=audio 5000 RTP/AVP 0\r\n";
        assert_eq!(parse_sdp_direction(sdp), MediaDirection::SendRecv);
    }

    #[test]
    fn test_compute_hold_state() {
        assert_eq!(
            compute_hold_state(MediaDirection::SendRecv, MediaDirection::SendRecv),
            HoldState::Active
        );
        assert_eq!(
            compute_hold_state(MediaDirection::SendOnly, MediaDirection::RecvOnly),
            HoldState::LocalHold
        );
        assert_eq!(
            compute_hold_state(MediaDirection::RecvOnly, MediaDirection::SendOnly),
            HoldState::RemoteHold
        );
        assert_eq!(
            compute_hold_state(MediaDirection::Inactive, MediaDirection::Inactive),
            HoldState::BothHold
        );
    }
}
