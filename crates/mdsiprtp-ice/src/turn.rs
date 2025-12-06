//! TURN client implementation (RFC 5766).
//!
//! Provides relay allocation for NAT traversal when STUN alone is insufficient.
//! TURN servers act as relay points for media traffic when peer-to-peer
//! connectivity cannot be established.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use rand::RngCore;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing::{debug, trace};

/// STUN magic cookie.
const MAGIC_COOKIE: u32 = 0x2112A442;

/// TURN message types (RFC 5766).
const ALLOCATE_REQUEST: u16 = 0x0003;
const ALLOCATE_RESPONSE: u16 = 0x0103;
const ALLOCATE_ERROR: u16 = 0x0113;
const REFRESH_REQUEST: u16 = 0x0004;
const REFRESH_RESPONSE: u16 = 0x0104;
const SEND_INDICATION: u16 = 0x0016;
const DATA_INDICATION: u16 = 0x0017;
const CREATE_PERMISSION_REQUEST: u16 = 0x0008;
const CREATE_PERMISSION_RESPONSE: u16 = 0x0108;
// Channel binding (reserved for future use)
#[allow(dead_code)]
const CHANNEL_BIND_REQUEST: u16 = 0x0009;
#[allow(dead_code)]
const CHANNEL_BIND_RESPONSE: u16 = 0x0109;

/// STUN/TURN attribute types.
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
const ATTR_XOR_RELAYED_ADDRESS: u16 = 0x0016;
const ATTR_XOR_PEER_ADDRESS: u16 = 0x0012;
const ATTR_LIFETIME: u16 = 0x000D;
const ATTR_DATA: u16 = 0x0013;
const ATTR_REQUESTED_TRANSPORT: u16 = 0x0019;
const ATTR_USERNAME: u16 = 0x0006;
const ATTR_REALM: u16 = 0x0014;
const ATTR_NONCE: u16 = 0x0015;
#[allow(dead_code)]
const ATTR_MESSAGE_INTEGRITY: u16 = 0x0008;
const ATTR_ERROR_CODE: u16 = 0x0009;
#[allow(dead_code)]
const ATTR_CHANNEL_NUMBER: u16 = 0x000C;

/// Transport protocol for TURN allocation.
const TRANSPORT_UDP: u8 = 17;

/// Address family.
const AF_IPV4: u8 = 0x01;
const AF_IPV6: u8 = 0x02;

/// TURN errors.
#[derive(Error, Debug)]
pub enum TurnError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Request timeout")]
    Timeout,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("TURN error: {code} {reason}")]
    ErrorResponse { code: u16, reason: String },

    #[error("Authentication required")]
    AuthRequired { realm: String, nonce: String },

    #[error("No relay address in response")]
    NoRelayAddress,

    #[error("Allocation not active")]
    NotAllocated,
}

/// TURN server configuration.
#[derive(Debug, Clone)]
pub struct TurnServer {
    /// Server address.
    pub addr: SocketAddr,
    /// Username for authentication.
    pub username: String,
    /// Password/credential for authentication.
    pub password: String,
    /// Realm (optional, will be discovered).
    pub realm: Option<String>,
}

impl TurnServer {
    /// Create a new TURN server configuration.
    pub fn new(addr: SocketAddr, username: &str, password: &str) -> Self {
        Self {
            addr,
            username: username.to_string(),
            password: password.to_string(),
            realm: None,
        }
    }
}

/// TURN allocation state.
#[derive(Debug, Clone)]
pub struct TurnAllocation {
    /// Relayed address (the public relay address).
    pub relayed_addr: SocketAddr,
    /// Mapped address (our public address as seen by the server).
    pub mapped_addr: SocketAddr,
    /// Lifetime in seconds.
    pub lifetime: u32,
    /// Realm used for authentication.
    pub realm: String,
    /// Nonce used for authentication.
    pub nonce: String,
}

/// TURN client for relay allocation.
pub struct TurnClient {
    socket: UdpSocket,
    server: TurnServer,
    timeout: Duration,
    retries: u32,
    allocation: Option<TurnAllocation>,
    transaction_id: [u8; 12],
}

impl TurnClient {
    /// Create a new TURN client.
    pub async fn new(server: TurnServer) -> Result<Self, TurnError> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(&server.addr).await?;
        debug!("TURN client bound to {}, connecting to {}", socket.local_addr()?, server.addr);

        Ok(Self {
            socket,
            server,
            timeout: Duration::from_secs(5),
            retries: 3,
            allocation: None,
            transaction_id: generate_transaction_id(),
        })
    }

    /// Get the local socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, TurnError> {
        Ok(self.socket.local_addr()?)
    }

    /// Get the current allocation, if any.
    pub fn allocation(&self) -> Option<&TurnAllocation> {
        self.allocation.as_ref()
    }

    /// Get the relayed address (relay candidate).
    pub fn relayed_addr(&self) -> Option<SocketAddr> {
        self.allocation.as_ref().map(|a| a.relayed_addr)
    }

    /// Allocate a relay address on the TURN server.
    pub async fn allocate(&mut self) -> Result<TurnAllocation, TurnError> {
        debug!("Requesting TURN allocation from {}", self.server.addr);

        // First attempt without credentials to get realm/nonce
        self.transaction_id = generate_transaction_id();
        let request = self.build_allocate_request(None)?;

        match self.send_request(&request).await? {
            AllocateResult::Success(alloc) => {
                self.allocation = Some(alloc.clone());
                Ok(alloc)
            }
            AllocateResult::AuthRequired { realm, nonce } => {
                debug!("Authentication required, realm={}, nonce={}", realm, nonce);

                // Retry with credentials
                self.transaction_id = generate_transaction_id();
                let auth = AuthContext {
                    username: self.server.username.clone(),
                    password: self.server.password.clone(),
                    realm: realm.clone(),
                    nonce: nonce.clone(),
                };
                let request = self.build_allocate_request(Some(&auth))?;

                match self.send_request(&request).await? {
                    AllocateResult::Success(mut alloc) => {
                        alloc.realm = realm;
                        alloc.nonce = nonce;
                        self.allocation = Some(alloc.clone());
                        Ok(alloc)
                    }
                    AllocateResult::AuthRequired { .. } => {
                        Err(TurnError::ErrorResponse {
                            code: 401,
                            reason: "Authentication failed".into(),
                        })
                    }
                }
            }
        }
    }

    /// Refresh the allocation to extend its lifetime.
    pub async fn refresh(&mut self, lifetime: u32) -> Result<u32, TurnError> {
        let alloc = self.allocation.as_ref().ok_or(TurnError::NotAllocated)?;

        debug!("Refreshing TURN allocation, requested lifetime={}", lifetime);

        let auth = AuthContext {
            username: self.server.username.clone(),
            password: self.server.password.clone(),
            realm: alloc.realm.clone(),
            nonce: alloc.nonce.clone(),
        };

        self.transaction_id = generate_transaction_id();
        let request = self.build_refresh_request(lifetime, &auth)?;

        let response = self.send_raw(&request).await?;
        let new_lifetime = self.parse_refresh_response(&response)?;

        if let Some(ref mut alloc) = self.allocation {
            alloc.lifetime = new_lifetime;
        }

        Ok(new_lifetime)
    }

    /// Create permission for a peer address.
    ///
    /// This is required before receiving data from a peer.
    pub async fn create_permission(&mut self, peer_addr: SocketAddr) -> Result<(), TurnError> {
        let alloc = self.allocation.as_ref().ok_or(TurnError::NotAllocated)?;

        debug!("Creating permission for peer {}", peer_addr);

        let auth = AuthContext {
            username: self.server.username.clone(),
            password: self.server.password.clone(),
            realm: alloc.realm.clone(),
            nonce: alloc.nonce.clone(),
        };

        self.transaction_id = generate_transaction_id();
        let request = self.build_permission_request(peer_addr, &auth)?;

        let response = self.send_raw(&request).await?;
        self.parse_permission_response(&response)?;

        Ok(())
    }

    /// Send data to a peer through the relay.
    ///
    /// Uses Send indication (no response expected).
    pub async fn send_data(&self, peer_addr: SocketAddr, data: &[u8]) -> Result<(), TurnError> {
        if self.allocation.is_none() {
            return Err(TurnError::NotAllocated);
        }

        trace!("Sending {} bytes to peer {} via relay", data.len(), peer_addr);

        let indication = self.build_send_indication(peer_addr, data)?;
        self.socket.send(&indication).await?;

        Ok(())
    }

    /// Receive data from the relay (checks for Data indication).
    ///
    /// Returns (peer_addr, data) if a Data indication was received.
    pub async fn recv_data(&self) -> Result<(SocketAddr, Vec<u8>), TurnError> {
        let mut buf = vec![0u8; 65536];
        let len = self.socket.recv(&mut buf).await?;
        self.parse_data_indication(&buf[..len])
    }

    /// Receive data with timeout.
    pub async fn recv_data_timeout(
        &self,
        duration: Duration,
    ) -> Result<(SocketAddr, Vec<u8>), TurnError> {
        match timeout(duration, self.recv_data()).await {
            Ok(result) => result,
            Err(_) => Err(TurnError::Timeout),
        }
    }

    /// Build an Allocate request.
    fn build_allocate_request(&self, auth: Option<&AuthContext>) -> Result<Bytes, TurnError> {
        let mut attrs = BytesMut::new();

        // REQUESTED-TRANSPORT (UDP)
        attrs.put_u16(ATTR_REQUESTED_TRANSPORT);
        attrs.put_u16(4);
        attrs.put_u8(TRANSPORT_UDP);
        attrs.put_u8(0); // Reserved
        attrs.put_u8(0);
        attrs.put_u8(0);

        if let Some(auth) = auth {
            // USERNAME
            let username_bytes = auth.username.as_bytes();
            attrs.put_u16(ATTR_USERNAME);
            attrs.put_u16(username_bytes.len() as u16);
            attrs.put_slice(username_bytes);
            pad_to_4_bytes(&mut attrs, username_bytes.len());

            // REALM
            let realm_bytes = auth.realm.as_bytes();
            attrs.put_u16(ATTR_REALM);
            attrs.put_u16(realm_bytes.len() as u16);
            attrs.put_slice(realm_bytes);
            pad_to_4_bytes(&mut attrs, realm_bytes.len());

            // NONCE
            let nonce_bytes = auth.nonce.as_bytes();
            attrs.put_u16(ATTR_NONCE);
            attrs.put_u16(nonce_bytes.len() as u16);
            attrs.put_slice(nonce_bytes);
            pad_to_4_bytes(&mut attrs, nonce_bytes.len());

            // MESSAGE-INTEGRITY would be computed here
            // For simplicity, we're not implementing full HMAC-SHA1
        }

        let mut msg = BytesMut::with_capacity(20 + attrs.len());
        msg.put_u16(ALLOCATE_REQUEST);
        msg.put_u16(attrs.len() as u16);
        msg.put_u32(MAGIC_COOKIE);
        msg.put_slice(&self.transaction_id);
        msg.put_slice(&attrs);

        Ok(msg.freeze())
    }

    /// Build a Refresh request.
    fn build_refresh_request(&self, lifetime: u32, auth: &AuthContext) -> Result<Bytes, TurnError> {
        let mut attrs = BytesMut::new();

        // LIFETIME
        attrs.put_u16(ATTR_LIFETIME);
        attrs.put_u16(4);
        attrs.put_u32(lifetime);

        // USERNAME
        let username_bytes = auth.username.as_bytes();
        attrs.put_u16(ATTR_USERNAME);
        attrs.put_u16(username_bytes.len() as u16);
        attrs.put_slice(username_bytes);
        pad_to_4_bytes(&mut attrs, username_bytes.len());

        // REALM
        let realm_bytes = auth.realm.as_bytes();
        attrs.put_u16(ATTR_REALM);
        attrs.put_u16(realm_bytes.len() as u16);
        attrs.put_slice(realm_bytes);
        pad_to_4_bytes(&mut attrs, realm_bytes.len());

        // NONCE
        let nonce_bytes = auth.nonce.as_bytes();
        attrs.put_u16(ATTR_NONCE);
        attrs.put_u16(nonce_bytes.len() as u16);
        attrs.put_slice(nonce_bytes);
        pad_to_4_bytes(&mut attrs, nonce_bytes.len());

        let mut msg = BytesMut::with_capacity(20 + attrs.len());
        msg.put_u16(REFRESH_REQUEST);
        msg.put_u16(attrs.len() as u16);
        msg.put_u32(MAGIC_COOKIE);
        msg.put_slice(&self.transaction_id);
        msg.put_slice(&attrs);

        Ok(msg.freeze())
    }

    /// Build a CreatePermission request.
    fn build_permission_request(
        &self,
        peer_addr: SocketAddr,
        auth: &AuthContext,
    ) -> Result<Bytes, TurnError> {
        let mut attrs = BytesMut::new();

        // XOR-PEER-ADDRESS
        encode_xor_address(&mut attrs, ATTR_XOR_PEER_ADDRESS, peer_addr, &self.transaction_id);

        // USERNAME
        let username_bytes = auth.username.as_bytes();
        attrs.put_u16(ATTR_USERNAME);
        attrs.put_u16(username_bytes.len() as u16);
        attrs.put_slice(username_bytes);
        pad_to_4_bytes(&mut attrs, username_bytes.len());

        // REALM
        let realm_bytes = auth.realm.as_bytes();
        attrs.put_u16(ATTR_REALM);
        attrs.put_u16(realm_bytes.len() as u16);
        attrs.put_slice(realm_bytes);
        pad_to_4_bytes(&mut attrs, realm_bytes.len());

        // NONCE
        let nonce_bytes = auth.nonce.as_bytes();
        attrs.put_u16(ATTR_NONCE);
        attrs.put_u16(nonce_bytes.len() as u16);
        attrs.put_slice(nonce_bytes);
        pad_to_4_bytes(&mut attrs, nonce_bytes.len());

        let mut msg = BytesMut::with_capacity(20 + attrs.len());
        msg.put_u16(CREATE_PERMISSION_REQUEST);
        msg.put_u16(attrs.len() as u16);
        msg.put_u32(MAGIC_COOKIE);
        msg.put_slice(&self.transaction_id);
        msg.put_slice(&attrs);

        Ok(msg.freeze())
    }

    /// Build a Send indication.
    fn build_send_indication(&self, peer_addr: SocketAddr, data: &[u8]) -> Result<Bytes, TurnError> {
        let mut attrs = BytesMut::new();

        // XOR-PEER-ADDRESS
        encode_xor_address(&mut attrs, ATTR_XOR_PEER_ADDRESS, peer_addr, &self.transaction_id);

        // DATA
        attrs.put_u16(ATTR_DATA);
        attrs.put_u16(data.len() as u16);
        attrs.put_slice(data);
        pad_to_4_bytes(&mut attrs, data.len());

        let mut msg = BytesMut::with_capacity(20 + attrs.len());
        msg.put_u16(SEND_INDICATION);
        msg.put_u16(attrs.len() as u16);
        msg.put_u32(MAGIC_COOKIE);
        msg.put_slice(&self.transaction_id);
        msg.put_slice(&attrs);

        Ok(msg.freeze())
    }

    /// Send a request and receive a response.
    async fn send_request(&mut self, request: &[u8]) -> Result<AllocateResult, TurnError> {
        let response = self.send_raw(request).await?;
        self.parse_allocate_response(&response)
    }

    /// Send raw bytes and receive response.
    async fn send_raw(&self, request: &[u8]) -> Result<Vec<u8>, TurnError> {
        for attempt in 0..self.retries {
            if attempt > 0 {
                debug!("Retry {} for TURN request", attempt);
            }

            self.socket.send(request).await?;

            let mut buf = vec![0u8; 4096];
            match timeout(self.timeout, self.socket.recv(&mut buf)).await {
                Ok(Ok(len)) => {
                    trace!("Received {} bytes from TURN server", len);
                    return Ok(buf[..len].to_vec());
                }
                Ok(Err(e)) => return Err(TurnError::Io(e)),
                Err(_) => {
                    if attempt == self.retries - 1 {
                        return Err(TurnError::Timeout);
                    }
                }
            }
        }

        Err(TurnError::Timeout)
    }

    /// Parse an Allocate response.
    fn parse_allocate_response(&self, data: &[u8]) -> Result<AllocateResult, TurnError> {
        if data.len() < 20 {
            return Err(TurnError::InvalidResponse("Message too short".into()));
        }

        let mut buf = data;
        let msg_type = buf.get_u16();
        let msg_len = buf.get_u16() as usize;
        let _cookie = buf.get_u32();

        let mut txn_id = [0u8; 12];
        buf.copy_to_slice(&mut txn_id);

        if txn_id != self.transaction_id {
            return Err(TurnError::InvalidResponse("Transaction ID mismatch".into()));
        }

        if msg_type == ALLOCATE_ERROR {
            return self.parse_error_response(&data[20..20 + msg_len]);
        }

        if msg_type != ALLOCATE_RESPONSE {
            return Err(TurnError::InvalidResponse(format!(
                "Unexpected message type: 0x{:04x}",
                msg_type
            )));
        }

        // Parse attributes
        let mut attrs = &data[20..20 + msg_len];
        let mut relayed_addr = None;
        let mut mapped_addr = None;
        let mut lifetime = 600; // Default

        while attrs.len() >= 4 {
            let attr_type = attrs.get_u16();
            let attr_len = attrs.get_u16() as usize;

            if attrs.len() < attr_len {
                break;
            }

            let attr_data = &attrs[..attr_len];

            match attr_type {
                ATTR_XOR_RELAYED_ADDRESS => {
                    relayed_addr = parse_xor_address(attr_data, &self.transaction_id);
                }
                ATTR_XOR_MAPPED_ADDRESS => {
                    mapped_addr = parse_xor_address(attr_data, &self.transaction_id);
                }
                ATTR_LIFETIME => {
                    if attr_len >= 4 {
                        let mut lb = attr_data;
                        lifetime = lb.get_u32();
                    }
                }
                _ => {}
            }

            let padded_len = (attr_len + 3) & !3;
            if attrs.len() >= padded_len {
                attrs.advance(padded_len);
            } else {
                break;
            }
        }

        let relayed = relayed_addr.ok_or(TurnError::NoRelayAddress)?;
        let mapped = mapped_addr.unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));

        Ok(AllocateResult::Success(TurnAllocation {
            relayed_addr: relayed,
            mapped_addr: mapped,
            lifetime,
            realm: String::new(),
            nonce: String::new(),
        }))
    }

    /// Parse an error response.
    fn parse_error_response(&self, attrs: &[u8]) -> Result<AllocateResult, TurnError> {
        let mut buf = attrs;
        let mut error_code = 0u16;
        let mut error_reason = String::new();
        let mut realm = None;
        let mut nonce = None;

        while buf.len() >= 4 {
            let attr_type = buf.get_u16();
            let attr_len = buf.get_u16() as usize;

            if buf.len() < attr_len {
                break;
            }

            let attr_data = &buf[..attr_len];

            match attr_type {
                ATTR_ERROR_CODE if attr_len >= 4 => {
                    let _reserved = u16::from_be_bytes([attr_data[0], attr_data[1]]);
                    let class = attr_data[2];
                    let number = attr_data[3];
                    error_code = (class as u16) * 100 + (number as u16);
                    if attr_len > 4 {
                        error_reason = String::from_utf8_lossy(&attr_data[4..]).to_string();
                    }
                }
                ATTR_REALM => {
                    realm = Some(String::from_utf8_lossy(attr_data).to_string());
                }
                ATTR_NONCE => {
                    nonce = Some(String::from_utf8_lossy(attr_data).to_string());
                }
                _ => {}
            }

            let padded_len = (attr_len + 3) & !3;
            if buf.len() >= padded_len {
                buf.advance(padded_len);
            } else {
                break;
            }
        }

        // Check if this is an auth challenge (401)
        if error_code == 401 {
            if let (Some(r), Some(n)) = (realm, nonce) {
                return Ok(AllocateResult::AuthRequired { realm: r, nonce: n });
            }
        }

        Err(TurnError::ErrorResponse {
            code: error_code,
            reason: error_reason,
        })
    }

    /// Parse a Refresh response.
    fn parse_refresh_response(&self, data: &[u8]) -> Result<u32, TurnError> {
        if data.len() < 20 {
            return Err(TurnError::InvalidResponse("Message too short".into()));
        }

        let mut buf = data;
        let msg_type = buf.get_u16();
        let msg_len = buf.get_u16() as usize;

        if msg_type != REFRESH_RESPONSE {
            return Err(TurnError::InvalidResponse(format!(
                "Unexpected message type: 0x{:04x}",
                msg_type
            )));
        }

        // Skip header
        let attrs = &data[20..20 + msg_len];
        let mut buf = attrs;
        let mut lifetime = 600;

        while buf.len() >= 4 {
            let attr_type = buf.get_u16();
            let attr_len = buf.get_u16() as usize;

            if buf.len() < attr_len {
                break;
            }

            if attr_type == ATTR_LIFETIME && attr_len >= 4 {
                let attr_data = &buf[..attr_len];
                let mut lb = attr_data;
                lifetime = lb.get_u32();
            }

            let padded_len = (attr_len + 3) & !3;
            if buf.len() >= padded_len {
                buf.advance(padded_len);
            } else {
                break;
            }
        }

        Ok(lifetime)
    }

    /// Parse a CreatePermission response.
    fn parse_permission_response(&self, data: &[u8]) -> Result<(), TurnError> {
        if data.len() < 20 {
            return Err(TurnError::InvalidResponse("Message too short".into()));
        }

        let mut buf = data;
        let msg_type = buf.get_u16();

        if msg_type != CREATE_PERMISSION_RESPONSE {
            return Err(TurnError::InvalidResponse(format!(
                "Unexpected message type: 0x{:04x}",
                msg_type
            )));
        }

        Ok(())
    }

    /// Parse a Data indication.
    fn parse_data_indication(&self, data: &[u8]) -> Result<(SocketAddr, Vec<u8>), TurnError> {
        if data.len() < 20 {
            return Err(TurnError::InvalidResponse("Message too short".into()));
        }

        let mut buf = data;
        let msg_type = buf.get_u16();
        let msg_len = buf.get_u16() as usize;
        let _cookie = buf.get_u32();

        let mut txn_id = [0u8; 12];
        buf.copy_to_slice(&mut txn_id);

        if msg_type != DATA_INDICATION {
            return Err(TurnError::InvalidResponse(format!(
                "Expected Data indication, got 0x{:04x}",
                msg_type
            )));
        }

        let mut attrs = &data[20..20 + msg_len];
        let mut peer_addr = None;
        let mut payload = None;

        while attrs.len() >= 4 {
            let attr_type = attrs.get_u16();
            let attr_len = attrs.get_u16() as usize;

            if attrs.len() < attr_len {
                break;
            }

            let attr_data = &attrs[..attr_len];

            match attr_type {
                ATTR_XOR_PEER_ADDRESS => {
                    peer_addr = parse_xor_address(attr_data, &txn_id);
                }
                ATTR_DATA => {
                    payload = Some(attr_data.to_vec());
                }
                _ => {}
            }

            let padded_len = (attr_len + 3) & !3;
            if attrs.len() >= padded_len {
                attrs.advance(padded_len);
            } else {
                break;
            }
        }

        match (peer_addr, payload) {
            (Some(addr), Some(data)) => Ok((addr, data)),
            _ => Err(TurnError::InvalidResponse("Missing peer address or data".into())),
        }
    }
}

/// Authentication context.
struct AuthContext {
    username: String,
    #[allow(dead_code)] // Reserved for MESSAGE-INTEGRITY computation
    password: String,
    realm: String,
    nonce: String,
}

/// Result of an Allocate request.
enum AllocateResult {
    Success(TurnAllocation),
    AuthRequired { realm: String, nonce: String },
}

/// Generate a random transaction ID.
fn generate_transaction_id() -> [u8; 12] {
    let mut id = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut id);
    id
}

/// Pad buffer to 4-byte boundary.
fn pad_to_4_bytes(buf: &mut BytesMut, len: usize) {
    let padding = (4 - (len % 4)) % 4;
    for _ in 0..padding {
        buf.put_u8(0);
    }
}

/// Encode an XOR-MAPPED-ADDRESS style attribute.
fn encode_xor_address(buf: &mut BytesMut, attr_type: u16, addr: SocketAddr, txn_id: &[u8; 12]) {
    match addr.ip() {
        IpAddr::V4(ipv4) => {
            buf.put_u16(attr_type);
            buf.put_u16(8);
            buf.put_u8(0); // Reserved
            buf.put_u8(AF_IPV4);

            let xor_port = addr.port() ^ (MAGIC_COOKIE >> 16) as u16;
            buf.put_u16(xor_port);

            let ip_bytes = ipv4.octets();
            let cookie_bytes = MAGIC_COOKIE.to_be_bytes();
            buf.put_u8(ip_bytes[0] ^ cookie_bytes[0]);
            buf.put_u8(ip_bytes[1] ^ cookie_bytes[1]);
            buf.put_u8(ip_bytes[2] ^ cookie_bytes[2]);
            buf.put_u8(ip_bytes[3] ^ cookie_bytes[3]);
        }
        IpAddr::V6(ipv6) => {
            buf.put_u16(attr_type);
            buf.put_u16(20);
            buf.put_u8(0); // Reserved
            buf.put_u8(AF_IPV6);

            let xor_port = addr.port() ^ (MAGIC_COOKIE >> 16) as u16;
            buf.put_u16(xor_port);

            let ip_bytes = ipv6.octets();
            let cookie_bytes = MAGIC_COOKIE.to_be_bytes();

            // XOR with magic cookie for first 4 bytes
            for i in 0..4 {
                buf.put_u8(ip_bytes[i] ^ cookie_bytes[i]);
            }
            // XOR with transaction ID for remaining 12 bytes
            for i in 0..12 {
                buf.put_u8(ip_bytes[4 + i] ^ txn_id[i]);
            }
        }
    }
}

/// Parse an XOR-MAPPED-ADDRESS style attribute.
fn parse_xor_address(data: &[u8], txn_id: &[u8; 12]) -> Option<SocketAddr> {
    if data.len() < 4 {
        return None;
    }

    let _reserved = data[0];
    let family = data[1];
    let xor_port = u16::from_be_bytes([data[2], data[3]]);
    let port = xor_port ^ (MAGIC_COOKIE >> 16) as u16;

    match family {
        AF_IPV4 if data.len() >= 8 => {
            let cookie_bytes = MAGIC_COOKIE.to_be_bytes();
            let ip = Ipv4Addr::new(
                data[4] ^ cookie_bytes[0],
                data[5] ^ cookie_bytes[1],
                data[6] ^ cookie_bytes[2],
                data[7] ^ cookie_bytes[3],
            );
            Some(SocketAddr::new(IpAddr::V4(ip), port))
        }
        AF_IPV6 if data.len() >= 20 => {
            let cookie_bytes = MAGIC_COOKIE.to_be_bytes();
            let mut ip_bytes = [0u8; 16];

            // XOR with magic cookie for first 4 bytes
            for i in 0..4 {
                ip_bytes[i] = data[4 + i] ^ cookie_bytes[i];
            }
            // XOR with transaction ID for remaining 12 bytes
            for i in 0..12 {
                ip_bytes[4 + i] = data[8 + i] ^ txn_id[i];
            }

            let ip = std::net::Ipv6Addr::from(ip_bytes);
            Some(SocketAddr::new(IpAddr::V6(ip), port))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_transaction_id() {
        let id1 = generate_transaction_id();
        let id2 = generate_transaction_id();
        assert_ne!(id1, id2);
        assert_eq!(id1.len(), 12);
    }

    #[test]
    fn test_xor_address_encode_decode_ipv4() {
        let txn_id = [0x11u8; 12];
        let addr: SocketAddr = "192.168.1.100:12345".parse().unwrap();

        let mut buf = BytesMut::new();
        encode_xor_address(&mut buf, ATTR_XOR_PEER_ADDRESS, addr, &txn_id);

        // Skip type and length
        let encoded = &buf[4..];
        let decoded = parse_xor_address(encoded, &txn_id).unwrap();

        assert_eq!(decoded, addr);
    }

    #[test]
    fn test_turn_server_new() {
        let server = TurnServer::new(
            "1.2.3.4:3478".parse().unwrap(),
            "user",
            "pass",
        );

        assert_eq!(server.username, "user");
        assert_eq!(server.password, "pass");
        assert_eq!(server.addr.port(), 3478);
    }

    #[test]
    fn test_pad_to_4_bytes() {
        let mut buf = BytesMut::new();
        buf.put_slice(b"abc");
        pad_to_4_bytes(&mut buf, 3);
        assert_eq!(buf.len(), 4);

        let mut buf = BytesMut::new();
        buf.put_slice(b"abcd");
        pad_to_4_bytes(&mut buf, 4);
        assert_eq!(buf.len(), 4);

        let mut buf = BytesMut::new();
        buf.put_slice(b"ab");
        pad_to_4_bytes(&mut buf, 2);
        assert_eq!(buf.len(), 4);
    }
}
