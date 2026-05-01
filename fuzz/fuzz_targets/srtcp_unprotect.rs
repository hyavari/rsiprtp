#![no_main]
use libfuzzer_sys::fuzz_target;
use rsiprtp::srtp::{CryptoSuite, SrtcpContext};

const KEY: [u8; 16] = [0u8; 16];
const SALT: [u8; 14] = [0u8; 14];

fuzz_target!(|data: &[u8]| {
    let mut ctx = SrtcpContext::new(CryptoSuite::AesCm128HmacSha1_80, &KEY, &SALT)
        .expect("static key/salt are well-formed");
    let _ = ctx.unprotect(data);
});
