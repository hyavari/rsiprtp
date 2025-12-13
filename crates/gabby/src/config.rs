//! Configuration types for Gabby voice AI agent.

use serde::Deserialize;
use std::path::PathBuf;

/// Top-level configuration for Gabby.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GabbyConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub stt: SttConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub tts: TtsConfig,
    #[serde(default)]
    pub vad: VadConfig,
}

impl GabbyConfig {
    /// Load configuration from a TOML file.
    pub fn from_file(path: &std::path::Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadError(path.to_path_buf(), e))?;
        toml::from_str(&content).map_err(ConfigError::ParseError)
    }

    /// Load configuration from file if it exists, otherwise use defaults.
    /// Validates configuration after loading.
    pub fn load_or_default(path: &std::path::Path) -> Result<Self, ConfigError> {
        let config = if path.exists() {
            Self::from_file(path)?
        } else {
            tracing::info!("Config file {:?} not found, using defaults", path);
            Self::default()
        };
        config.validate()?;
        Ok(config)
    }

    /// Validate configuration values.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // VAD thresholds
        if self.vad.silence_threshold < 0.0 || self.vad.silence_threshold > 1.0 {
            return Err(ConfigError::InvalidValue(
                "vad.silence_threshold must be between 0.0 and 1.0".to_string(),
            ));
        }

        // Server ports
        if self.server.sip_port == 0 {
            return Err(ConfigError::InvalidValue(
                "server.sip_port cannot be 0".to_string(),
            ));
        }
        if self.server.rtp_port_start == 0 {
            return Err(ConfigError::InvalidValue(
                "server.rtp_port_start cannot be 0".to_string(),
            ));
        }

        // LLM settings
        if self.llm.temperature < 0.0 || self.llm.temperature > 2.0 {
            return Err(ConfigError::InvalidValue(
                "llm.temperature must be between 0.0 and 2.0".to_string(),
            ));
        }
        if self.llm.max_tokens == 0 {
            return Err(ConfigError::InvalidValue(
                "llm.max_tokens must be greater than 0".to_string(),
            ));
        }

        // VAD timing
        if self.vad.silence_duration_ms == 0 {
            return Err(ConfigError::InvalidValue(
                "vad.silence_duration_ms must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }
}

/// Server configuration for SIP/RTP.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Host to bind SIP socket to.
    #[serde(default = "default_sip_host")]
    pub sip_host: String,
    /// Port to listen for SIP messages.
    #[serde(default = "default_sip_port")]
    pub sip_port: u16,
    /// Starting port for RTP streams.
    #[serde(default = "default_rtp_port_start")]
    pub rtp_port_start: u16,
    /// Public IP to advertise in SDP (auto-detect if not set).
    pub public_ip: Option<String>,
    /// Call timeout in seconds (default: 300 = 5 minutes).
    #[serde(default)]
    pub call_timeout_secs: Option<u64>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            sip_host: default_sip_host(),
            sip_port: default_sip_port(),
            rtp_port_start: default_rtp_port_start(),
            public_ip: None,
            call_timeout_secs: None,
        }
    }
}

fn default_sip_host() -> String {
    "0.0.0.0".to_string()
}

fn default_sip_port() -> u16 {
    5060
}

fn default_rtp_port_start() -> u16 {
    10000
}

/// Speech-to-Text configuration (Vosk).
#[derive(Debug, Clone, Deserialize)]
pub struct SttConfig {
    /// Path to Vosk model directory.
    #[serde(default = "default_vosk_model_path")]
    pub model_path: PathBuf,
    /// Sample rate for STT (Vosk expects 16000).
    #[serde(default = "default_stt_sample_rate")]
    pub sample_rate: u32,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model_path: default_vosk_model_path(),
            sample_rate: default_stt_sample_rate(),
        }
    }
}

fn default_vosk_model_path() -> PathBuf {
    PathBuf::from("./models/vosk-model-small-en-us-0.15")
}

fn default_stt_sample_rate() -> u32 {
    16000
}

/// LLM configuration (Ollama).
#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    /// Ollama API endpoint.
    #[serde(default = "default_llm_endpoint")]
    pub endpoint: String,
    /// Model to use.
    #[serde(default = "default_llm_model")]
    pub model: String,
    /// System prompt defining Gabby's personality.
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    /// Temperature for response generation.
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    /// Maximum tokens in response.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Timeout for LLM requests in seconds.
    #[serde(default = "default_llm_timeout")]
    pub timeout_secs: u64,
    /// Maximum number of messages to keep in conversation history.
    #[serde(default = "default_max_history_messages")]
    pub max_history_messages: usize,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            endpoint: default_llm_endpoint(),
            model: default_llm_model(),
            system_prompt: default_system_prompt(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            timeout_secs: default_llm_timeout(),
            max_history_messages: default_max_history_messages(),
        }
    }
}

fn default_llm_endpoint() -> String {
    "http://localhost:11434".to_string()
}

fn default_llm_model() -> String {
    "llama3.2:3b".to_string()
}

fn default_system_prompt() -> String {
    r#"You are Gabby, a friendly and helpful voice assistant.
Keep responses concise and natural for spoken conversation.
Avoid markdown, bullet points, or formatting.
Respond in 1-3 sentences."#
        .to_string()
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tokens() -> u32 {
    150
}

fn default_llm_timeout() -> u64 {
    30
}

fn default_max_history_messages() -> usize {
    20
}

/// Text-to-Speech configuration (Piper).
#[derive(Debug, Clone, Deserialize)]
pub struct TtsConfig {
    /// Path to Piper binary.
    #[serde(default = "default_piper_binary")]
    pub piper_binary: PathBuf,
    /// Path to Piper ONNX model.
    #[serde(default = "default_piper_model")]
    pub model_path: PathBuf,
    /// Path to Piper model config JSON.
    #[serde(default = "default_piper_config")]
    pub config_path: PathBuf,
    /// Output sample rate of Piper model.
    #[serde(default = "default_tts_sample_rate")]
    pub sample_rate: u32,
    /// Timeout for TTS synthesis in seconds.
    #[serde(default = "default_tts_timeout")]
    pub timeout_secs: u64,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            piper_binary: default_piper_binary(),
            model_path: default_piper_model(),
            config_path: default_piper_config(),
            sample_rate: default_tts_sample_rate(),
            timeout_secs: default_tts_timeout(),
        }
    }
}

fn default_piper_binary() -> PathBuf {
    PathBuf::from("/usr/local/bin/piper")
}

fn default_piper_model() -> PathBuf {
    PathBuf::from("./models/en_US-amy-medium.onnx")
}

fn default_piper_config() -> PathBuf {
    PathBuf::from("./models/en_US-amy-medium.onnx.json")
}

fn default_tts_sample_rate() -> u32 {
    22050
}

fn default_tts_timeout() -> u64 {
    60
}

/// Voice Activity Detection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct VadConfig {
    /// RMS energy threshold for silence detection (0.0-1.0).
    #[serde(default = "default_silence_threshold")]
    pub silence_threshold: f32,
    /// Duration of silence (ms) to consider end of utterance.
    #[serde(default = "default_silence_duration_ms")]
    pub silence_duration_ms: u64,
    /// Minimum utterance duration (ms) before accepting.
    #[serde(default = "default_min_utterance_ms")]
    pub min_utterance_ms: u64,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            silence_threshold: default_silence_threshold(),
            silence_duration_ms: default_silence_duration_ms(),
            min_utterance_ms: default_min_utterance_ms(),
        }
    }
}

fn default_silence_threshold() -> f32 {
    0.02
}

fn default_silence_duration_ms() -> u64 {
    700
}

fn default_min_utterance_ms() -> u64 {
    500
}

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file {0}: {1}")]
    ReadError(PathBuf, std::io::Error),
    #[error("Failed to parse config: {0}")]
    ParseError(#[from] toml::de::Error),
    #[error("Invalid configuration: {0}")]
    InvalidValue(String),
}
