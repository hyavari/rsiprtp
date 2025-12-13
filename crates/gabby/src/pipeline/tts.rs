//! Text-to-Speech processor using Piper.
//!
//! Uses Piper as a subprocess for reliable TTS synthesis.

use crate::config::TtsConfig;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Text-to-Speech processor wrapping Piper.
pub struct TtsProcessor {
    piper_binary: PathBuf,
    model_path: PathBuf,
    config_path: PathBuf,
    sample_rate: u32,
    timeout: Duration,
}

impl TtsProcessor {
    /// Create a new TTS processor from configuration.
    pub fn new(config: &TtsConfig) -> Result<Self, TtsError> {
        // Verify piper binary exists
        if !config.piper_binary.exists() {
            return Err(TtsError::BinaryNotFound(config.piper_binary.clone()));
        }

        // Verify model exists
        if !config.model_path.exists() {
            return Err(TtsError::ModelNotFound(config.model_path.clone()));
        }

        Ok(Self {
            piper_binary: config.piper_binary.clone(),
            model_path: config.model_path.clone(),
            config_path: config.config_path.clone(),
            sample_rate: config.sample_rate,
            timeout: Duration::from_secs(config.timeout_secs),
        })
    }

    /// Synthesize text to audio samples.
    ///
    /// Returns raw PCM samples at the model's sample rate.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<i16>, TtsError> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Build command
        let mut cmd = Command::new(&self.piper_binary);
        cmd.arg("--model")
            .arg(&self.model_path)
            .arg("--output-raw")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()); // Capture stderr for debugging

        // Add config path if it exists
        if self.config_path.exists() {
            cmd.arg("--config").arg(&self.config_path);
        }

        let mut child = cmd.spawn().map_err(TtsError::SpawnFailed)?;

        // Write text to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .await
                .map_err(TtsError::IoError)?;
            // stdin is dropped here, closing it
        }

        // Wait for process with timeout
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| TtsError::Timeout)?
            .map_err(TtsError::IoError)?;

        // Log stderr if not empty (for debugging)
        if !output.stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::debug!("Piper stderr: {}", stderr.trim());
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TtsError::SynthesisFailed(
                output.status.code().unwrap_or(-1),
                stderr.to_string(),
            ));
        }

        // Convert bytes to i16 samples (little-endian)
        let samples: Vec<i16> = output
            .stdout
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        tracing::debug!(
            "Synthesized {} samples ({:.2}s) for text: {}",
            samples.len(),
            samples.len() as f32 / self.sample_rate as f32,
            &text[..text.len().min(50)]
        );

        Ok(samples)
    }

    /// Get the output sample rate of the TTS model.
    #[allow(dead_code)] // API for callers that need sample rate info
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Check if Piper is working correctly.
    #[allow(dead_code)] // API for startup health check
    pub async fn health_check(&self) -> bool {
        match self.synthesize("test").await {
            Ok(samples) => !samples.is_empty(),
            Err(_) => false,
        }
    }
}

/// Create a TTS processor that may be unavailable.
///
/// Returns None if Piper is not installed, allowing graceful degradation.
pub fn try_create_tts(config: &TtsConfig) -> Option<TtsProcessor> {
    match TtsProcessor::new(config) {
        Ok(tts) => Some(tts),
        Err(e) => {
            tracing::warn!("TTS unavailable: {}", e);
            None
        }
    }
}

/// TTS-related errors.
#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("Piper binary not found at {0}")]
    BinaryNotFound(PathBuf),

    #[error("Piper model not found at {0}")]
    ModelNotFound(PathBuf),

    #[error("Failed to spawn Piper process: {0}")]
    SpawnFailed(std::io::Error),

    #[error("IO error: {0}")]
    IoError(std::io::Error),

    #[error("TTS synthesis failed with exit code {0}: {1}")]
    SynthesisFailed(i32, String),

    #[error("TTS synthesis timed out")]
    Timeout,
}
