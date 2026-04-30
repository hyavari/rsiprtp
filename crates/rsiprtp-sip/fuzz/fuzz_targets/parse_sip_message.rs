#![no_main]

//! Fuzz target for `rsiprtp_sip::SipMessage::parse`.
//!
//! Drives the public SIP message parser with arbitrary bytes. The parser must
//! never panic, abort, or take unbounded time on attacker-controlled input;
//! returning `Err` is the only acceptable failure mode.

use libfuzzer_sys::fuzz_target;
use rsiprtp_sip::SipMessage;

fuzz_target!(|data: &[u8]| {
    let _ = SipMessage::parse(data);
});
