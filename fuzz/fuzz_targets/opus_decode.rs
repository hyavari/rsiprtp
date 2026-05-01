#![no_main]
use libfuzzer_sys::fuzz_target;
use rsiprtp::media::OpusCodec;

fuzz_target!(|data: &[u8]| {
    // Single shared default-config codec; the FFI/SIMD paths in libopus do
    // not depend on key material so we don't need a fresh decoder per
    // iteration.  Swallow any internal panic at the FFI boundary.
    let mut codec = match OpusCodec::new() {
        Ok(c) => c,
        Err(_) => return,
    };
    let _ = codec.decode(data);
});
