#![no_main]
use libfuzzer_sys::fuzz_target;
use rsiprtp::media::{JitterBuffer, JitterBufferConfig};

// Each "record" in the byte stream is:
//   2 bytes: sequence (u16 BE)
//   4 bytes: timestamp (u32 BE)
//   2 bytes: sample count N (u16 BE, capped to 320)
//   N*2 bytes: samples (i16 LE)
// We push multiple records per fuzz iteration to exercise stateful behaviour
// (jitter estimate, reorder window, duplicate detection).
fuzz_target!(|data: &[u8]| {
    let mut buf = JitterBuffer::new(JitterBufferConfig::default());
    let mut cursor = data;
    let mut budget = 64usize; // cap pushes per input
    while cursor.len() >= 8 && budget > 0 {
        budget -= 1;
        let seq = u16::from_be_bytes([cursor[0], cursor[1]]);
        let ts = u32::from_be_bytes([cursor[2], cursor[3], cursor[4], cursor[5]]);
        let n_raw = u16::from_be_bytes([cursor[6], cursor[7]]);
        let n = (n_raw as usize).min(320);
        cursor = &cursor[8..];
        let bytes_needed = n * 2;
        if cursor.len() < bytes_needed {
            break;
        }
        let samples: Vec<i16> = cursor[..bytes_needed]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        cursor = &cursor[bytes_needed..];
        let _ = buf.push(seq, ts, samples);
    }
});
