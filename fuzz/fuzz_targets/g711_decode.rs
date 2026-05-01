#![no_main]
use libfuzzer_sys::fuzz_target;
use rsiprtp::media::G711Codec;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    // Branch on the first byte: even ⇒ mu-law, odd ⇒ A-law.
    let codec = if data[0] & 1 == 0 {
        G711Codec::pcmu()
    } else {
        G711Codec::pcma()
    };
    let _ = codec.decode(&data[1..]);
});
