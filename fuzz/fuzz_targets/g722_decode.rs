#![no_main]
use libfuzzer_sys::fuzz_target;
use rsiprtp::media::G722Codec;

fuzz_target!(|data: &[u8]| {
    let mut codec = G722Codec::new();
    let _ = codec.decode(data);
});
