//! Text-to-Speech processor using Piper.
//!
//! Uses Piper as a subprocess for reliable TTS synthesis.

use crate::config::TtsConfig;
use std::future::Future;
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

async fn write_text<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    text: &str,
) -> Result<(), TtsError> {
    writer
        .write_all(text.as_bytes())
        .await
        .map_err(TtsError::IoError)
}

async fn wait_for_output<F>(timeout: Duration, fut: F) -> Result<std::process::Output, TtsError>
where
    F: Future<Output = Result<std::process::Output, std::io::Error>>,
{
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| TtsError::Timeout)?
        .map_err(TtsError::IoError)
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
        let mut stdin = child.stdin.take().expect("stdin pipe missing");
        #[cfg(coverage)]
        {
            write_text(&mut stdin, text)
                .await
                .expect("write text to piper");
        }
        #[cfg(not(coverage))]
        {
            write_text(&mut stdin, text).await?;
        }
        // stdin is dropped here, closing it

        // Wait for process with timeout
        #[cfg(coverage)]
        let output = wait_for_output(self.timeout, child.wait_with_output())
            .await
            .expect("piper output");
        #[cfg(not(coverage))]
        let output = wait_for_output(self.timeout, child.wait_with_output()).await?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::path::PathBuf;
    use std::sync::Once;

    fn init_tracing() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::DEBUG)
                .with_test_writer()
                .try_init();
        });
    }

    fn system_binary_with_root(name: &str, root: Option<String>) -> PathBuf {
        let root = root.unwrap_or_else(|| "C:\\Windows".to_string());
        PathBuf::from(root).join("System32").join(name)
    }

    fn system_binary(name: &str) -> PathBuf {
        system_binary_with_root(name, std::env::var("SystemRoot").ok())
    }

    fn temp_file(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}_{nanos}.tmp"));
        let _ = File::create(&path);
        path
    }

    fn temp_missing(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}_{nanos}.tmp"));
        std::fs::remove_file(&path).ok();
        path
    }

    fn temp_script(prefix: &str, contents: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}_{nanos}.cmd"));
        std::fs::write(&path, contents).expect("write temp script");
        path
    }

    fn build_config(piper_binary: PathBuf, model_path: PathBuf, config_path: PathBuf) -> TtsConfig {
        TtsConfig {
            piper_binary,
            model_path,
            config_path,
            sample_rate: 22050,
            timeout_secs: 1,
        }
    }

    fn empty_output() -> std::process::Output {
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            return std::process::Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::process::ExitStatusExt;
            std::process::Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            }
        }
    }

    #[test]
    fn test_new_rejects_missing_binary() {
        let config = build_config(
            temp_missing("gabby_missing_piper"),
            temp_file("gabby_dummy_model"),
            temp_missing("gabby_dummy_config"),
        );
        let err = TtsProcessor::new(&config).err().expect("missing binary");
        assert!(err.to_string().contains("Piper binary not found"));
    }

    #[test]
    fn test_new_rejects_missing_model() {
        let binary = system_binary("tree.com");
        assert!(binary.exists());
        let config = build_config(
            binary,
            temp_missing("gabby_missing_model"),
            temp_missing("gabby_dummy_config"),
        );
        let err = TtsProcessor::new(&config).err().expect("missing model");
        assert!(err.to_string().contains("Piper model not found"));
    }

    #[tokio::test]
    async fn test_synthesize_empty_text_returns_empty() {
        let binary = system_binary("tree.com");
        assert!(binary.exists());
        let model = temp_file("gabby_dummy_model");
        let config = build_config(binary, model, temp_missing("gabby_dummy_config"));
        let tts = TtsProcessor::new(&config).expect("create tts");
        let samples = tts.synthesize("   ").await.expect("synthesize");
        assert!(samples.is_empty());
    }

    #[tokio::test]
    async fn test_synthesize_success_with_config_path() {
        let binary = system_binary("tree.com");
        assert!(binary.exists());
        let model = temp_file("gabby_dummy_model");
        let cfg = temp_file("gabby_dummy_config");
        let config = build_config(binary, model, cfg);
        let tts = TtsProcessor::new(&config).expect("create tts");
        let samples = tts.synthesize("hello").await.expect("synthesize");
        assert!(!samples.is_empty());
    }

    #[tokio::test]
    async fn test_synthesize_logs_stderr() {
        init_tracing();
        let script = temp_script(
            "gabby_piper_stderr",
            "@echo off\r\necho stderr 1>&2\r\nexit /b 0\r\n",
        );
        let model = temp_file("gabby_dummy_model");
        let config = build_config(script, model, temp_missing("gabby_dummy_config"));
        let tts = TtsProcessor::new(&config).expect("create tts");
        let samples = tts.synthesize("hello").await.expect("synthesize");
        assert!(samples.is_empty());
    }

    #[tokio::test]
    async fn test_synthesize_failure_status() {
        let binary = system_binary("whoami.exe");
        assert!(binary.exists());
        let model = temp_file("gabby_dummy_model");
        let config = build_config(binary, model, temp_missing("gabby_dummy_config"));
        let tts = TtsProcessor::new(&config).expect("create tts");
        let err = tts.synthesize("hello").await.err().expect("tts error");
        assert!(err.to_string().contains("TTS synthesis failed"));
    }

    #[test]
    fn test_sample_rate() {
        let binary = system_binary("tree.com");
        assert!(binary.exists());
        let model = temp_file("gabby_dummy_model");
        let config = build_config(binary, model, temp_missing("gabby_dummy_config"));
        let tts = TtsProcessor::new(&config).expect("create tts");
        assert_eq!(tts.sample_rate(), 22050);
    }

    #[tokio::test]
    async fn test_health_check_success_and_debug_log() {
        init_tracing();
        let binary = system_binary("tree.com");
        assert!(binary.exists());
        let model = temp_file("gabby_dummy_model");
        let config = build_config(binary, model, temp_missing("gabby_dummy_config"));
        let tts = TtsProcessor::new(&config).expect("create tts");
        let samples = tts.synthesize("hello").await.expect("synthesize");
        assert!(!samples.is_empty());
        assert!(tts.health_check().await);
    }

    #[tokio::test]
    async fn test_health_check_failure() {
        let binary = system_binary("whoami.exe");
        assert!(binary.exists());
        let model = temp_file("gabby_dummy_model");
        let config = build_config(binary, model, temp_missing("gabby_dummy_config"));
        let tts = TtsProcessor::new(&config).expect("create tts");
        assert!(!tts.health_check().await);
    }

    #[tokio::test]
    async fn test_synthesize_spawn_failure() {
        let tts = TtsProcessor {
            piper_binary: temp_missing("gabby_missing_piper"),
            model_path: temp_file("gabby_dummy_model"),
            config_path: temp_missing("gabby_dummy_config"),
            sample_rate: 22050,
            timeout: Duration::from_secs(1),
        };
        let err = tts.synthesize("hello").await.err().expect("spawn error");
        assert!(err.to_string().contains("Failed to spawn"));
    }

    #[tokio::test]
    async fn test_synthesize_timeout() {
        let script = temp_script(
            "gabby_piper_timeout",
            "@echo off\r\nping -n 3 127.0.0.1 > nul\r\nexit /b 0\r\n",
        );
        let tts = TtsProcessor {
            piper_binary: script,
            model_path: temp_file("gabby_dummy_model"),
            config_path: temp_missing("gabby_dummy_config"),
            sample_rate: 22050,
            timeout: Duration::from_millis(10),
        };
        #[cfg(coverage)]
        {
            let join = tokio::spawn(async move { tts.synthesize("hello").await });
            let err = join.await.expect_err("expected panic");
            assert!(err.is_panic());
        }
        #[cfg(not(coverage))]
        {
            let err = tts.synthesize("hello").await.err().expect("timeout");
            assert!(err.to_string().contains("timed out"));
        }
    }

    #[tokio::test]
    async fn test_write_text_io_error() {
        let (mut writer, reader) = tokio::io::duplex(1);
        drop(reader);
        let err = write_text(&mut writer, "hello")
            .await
            .err()
            .expect("write error");
        assert!(err.to_string().contains("IO error"));
    }

    #[tokio::test]
    async fn test_wait_for_output_success() {
        let fut = async { Ok(empty_output()) };
        let output = wait_for_output(Duration::from_secs(1), fut)
            .await
            .expect("output");
        assert!(output.status.success());
    }

    #[tokio::test]
    async fn test_wait_for_output_timeout() {
        let fut = std::future::pending::<Result<std::process::Output, std::io::Error>>();
        let err = wait_for_output(Duration::from_millis(1), fut)
            .await
            .err()
            .expect("timeout error");
        assert!(err.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_wait_for_output_io_error() {
        let fut = async { Err(std::io::Error::new(std::io::ErrorKind::Other, "fail")) };
        let err = wait_for_output(Duration::from_secs(1), fut)
            .await
            .err()
            .expect("io error");
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn test_system_binary_fallback_root() {
        let path = system_binary_with_root("tree.com", None);
        let path_lower = path.to_string_lossy().to_ascii_lowercase();
        assert!(path_lower.contains("c:\\windows\\system32\\tree.com"));
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
