#![no_main]
use libfuzzer_sys::fuzz_target;
use rsiprtp::srtp::{CryptoSuite, SrtpContext};

// Fixed test key/salt.  Bugs we are looking for here are length-handling,
// header-parse and ROC-tracking panics on hostile input — keys are not
// the variable.
const KEY: [u8; 16] = [0u8; 16];
const SALT: [u8; 14] = [0u8; 14];

fuzz_target!(|data: &[u8]| {
    let mut ctx = SrtpContext::new(CryptoSuite::AesCm128HmacSha1_80, &KEY, &SALT)
        .expect("static key/salt are well-formed");
    let _ = ctx.unprotect(data);
});
