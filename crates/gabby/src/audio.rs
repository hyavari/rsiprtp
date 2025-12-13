//! Audio utilities for sample rate conversion and processing.

#![allow(dead_code)] // Utility functions for future use

use rubato::{FftFixedIn, Resampler as RubatoResampler};

/// Audio resampler for converting between sample rates.
pub struct Resampler {
    resampler: FftFixedIn<f32>,
    input_rate: u32,
    output_rate: u32,
    chunk_size: usize,
    buffer: Vec<f32>,
}

impl Resampler {
    /// Create a new resampler.
    ///
    /// # Arguments
    /// * `from_rate` - Input sample rate
    /// * `to_rate` - Output sample rate
    /// * `chunk_size` - Number of samples to process at once
    pub fn new(from_rate: u32, to_rate: u32, chunk_size: usize) -> Result<Self, ResamplerError> {
        let resampler = FftFixedIn::<f32>::new(
            from_rate as usize,
            to_rate as usize,
            chunk_size,
            1, // sub-chunks
            1, // channels (mono)
        )
        .map_err(|e| ResamplerError::InitFailed(e.to_string()))?;

        Ok(Self {
            resampler,
            input_rate: from_rate,
            output_rate: to_rate,
            chunk_size,
            buffer: Vec::new(),
        })
    }

    /// Create a resampler for 8kHz to 16kHz conversion (RTP to Vosk).
    pub fn rtp_to_vosk() -> Result<Self, ResamplerError> {
        Self::new(8000, 16000, 160) // 20ms at 8kHz = 160 samples
    }

    /// Create a resampler for 22kHz to 8kHz conversion (Piper to RTP).
    pub fn piper_to_rtp() -> Result<Self, ResamplerError> {
        Self::new(22050, 8000, 441) // ~20ms at 22kHz
    }

    /// Create a resampler for 16kHz to 8kHz conversion (Vosk rate to RTP).
    pub fn vosk_to_rtp() -> Result<Self, ResamplerError> {
        Self::new(16000, 8000, 320) // 20ms at 16kHz = 320 samples
    }

    /// Process audio samples through the resampler.
    ///
    /// Buffers input and returns resampled output when enough samples are available.
    pub fn process(&mut self, samples: &[i16]) -> Vec<i16> {
        // Convert i16 to f32 and add to buffer
        for &s in samples {
            self.buffer.push(s as f32 / i16::MAX as f32);
        }

        let mut output = Vec::new();

        // Process complete chunks
        while self.buffer.len() >= self.chunk_size {
            let chunk: Vec<f32> = self.buffer.drain(..self.chunk_size).collect();

            match self.resampler.process(&[chunk], None) {
                Ok(resampled) => {
                    if !resampled.is_empty() && !resampled[0].is_empty() {
                        // Convert back to i16
                        for &s in &resampled[0] {
                            let clamped = s.clamp(-1.0, 1.0);
                            output.push((clamped * i16::MAX as f32) as i16);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Resampling error: {}", e);
                }
            }
        }

        output
    }

    /// Flush remaining samples in the buffer.
    pub fn flush(&mut self) -> Vec<i16> {
        if self.buffer.is_empty() {
            return Vec::new();
        }

        // Pad to chunk size
        while self.buffer.len() < self.chunk_size {
            self.buffer.push(0.0);
        }

        self.process(&[])
    }

    /// Reset the resampler state.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.resampler.reset();
    }

    /// Get input sample rate.
    pub fn input_rate(&self) -> u32 {
        self.input_rate
    }

    /// Get output sample rate.
    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }
}

/// Calculate RMS (Root Mean Square) energy of audio samples.
///
/// Returns a value between 0.0 and 1.0.
pub fn calculate_rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let sum_squares: f64 = samples
        .iter()
        .map(|&s| {
            let normalized = s as f64 / i16::MAX as f64;
            normalized * normalized
        })
        .sum();

    (sum_squares / samples.len() as f64).sqrt() as f32
}

/// Check if audio samples represent silence.
pub fn is_silence(samples: &[i16], threshold: f32) -> bool {
    calculate_rms(samples) < threshold
}

/// Generate silence samples.
pub fn generate_silence(num_samples: usize) -> Vec<i16> {
    vec![0i16; num_samples]
}

/// Find sentence boundary in text for streaming TTS.
///
/// Returns the byte index after the sentence-ending punctuation.
pub fn find_sentence_boundary(text: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();

    for (i, (byte_idx, c)) in chars.iter().enumerate() {
        if *c == '.' || *c == '!' || *c == '?' {
            // End of string
            if i == chars.len() - 1 {
                return Some(byte_idx + c.len_utf8());
            }
            // Followed by whitespace
            if i + 1 < chars.len() && chars[i + 1].1.is_whitespace() {
                return Some(byte_idx + c.len_utf8());
            }
        }
    }

    None
}

/// Resampler errors.
#[derive(Debug, thiserror::Error)]
pub enum ResamplerError {
    #[error("Failed to initialize resampler: {0}")]
    InitFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_rms() {
        let silence = vec![0i16; 160];
        assert!(calculate_rms(&silence) < 0.001);

        let loud: Vec<i16> = (0..160)
            .map(|i| if i % 2 == 0 { 16000 } else { -16000 })
            .collect();
        assert!(calculate_rms(&loud) > 0.4);
    }

    #[test]
    fn test_find_sentence_boundary() {
        assert_eq!(find_sentence_boundary("Hello."), Some(6));
        assert_eq!(find_sentence_boundary("Hello. World"), Some(6));
        assert_eq!(find_sentence_boundary("Hello"), None);
        assert_eq!(find_sentence_boundary("What? Yes!"), Some(5));
    }

    #[test]
    fn test_is_silence() {
        let silence = vec![0i16; 160];
        assert!(is_silence(&silence, 0.01));

        let noise: Vec<i16> = (0..160).map(|i| (i * 100) as i16).collect();
        assert!(!is_silence(&noise, 0.01));
    }
}
