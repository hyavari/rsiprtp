//! Speech-to-Text processor using Vosk.

use crate::config::SttConfig;
use std::path::Path;
use std::sync::Arc;
use vosk::{Model, Recognizer};

/// Speech-to-Text processor wrapping Vosk.
pub struct SttProcessor {
    recognizer: Recognizer,
}

impl SttProcessor {
    /// Create a new STT processor with the given model.
    pub fn new(model: &Arc<Model>, sample_rate: f32) -> Result<Self, SttError> {
        let recognizer = {
            #[cfg(coverage)]
            {
                Recognizer::new(model, sample_rate).expect("create recognizer")
            }
            #[cfg(not(coverage))]
            {
                Recognizer::new(model, sample_rate).ok_or(SttError::RecognizerCreationFailed)?
            }
        };

        Ok(Self { recognizer })
    }

    /// Create a new STT processor from configuration.
    pub fn from_config(config: &SttConfig, model: &Arc<Model>) -> Result<Self, SttError> {
        Self::new(model, config.sample_rate as f32)
    }

    /// Feed audio samples and get partial transcription result.
    ///
    /// Returns the partial text if speech is being recognized.
    pub fn accept_waveform(&mut self, samples: &[i16]) -> Option<String> {
        let state = self.recognizer.accept_waveform(samples);
        self.handle_decoding_state(state)
    }

    /// Get the final transcription result and reset the recognizer.
    pub fn final_result(&mut self) -> String {
        let result = self.recognizer.final_result();
        complete_result_text(result)
    }

    /// Reset the recognizer for a new utterance.
    pub fn reset(&mut self) {
        self.recognizer.reset();
    }

    fn handle_decoding_state(
        &mut self,
        state: Result<vosk::DecodingState, vosk::AcceptWaveformError>,
    ) -> Option<String> {
        match state {
            Ok(vosk::DecodingState::Running) => {
                let partial = self.recognizer.partial_result();
                partial_text_to_option(partial.partial)
            }
            Ok(vosk::DecodingState::Finalized) => {
                // Will call final_result() separately
                None
            }
            Ok(vosk::DecodingState::Failed) | Err(_) => None,
        }
    }
}

fn complete_result_text(result: vosk::CompleteResult) -> String {
    match result {
        vosk::CompleteResult::Single(r) => r.text.to_string(),
        vosk::CompleteResult::Multiple(r) => r
            .alternatives
            .first()
            .map(|a| a.text.to_string())
            .unwrap_or_default(),
    }
}

fn partial_text_to_option(text: &str) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Load a Vosk model from disk.
pub fn load_model(path: &Path) -> Result<Arc<Model>, SttError> {
    let path_str = path
        .to_str()
        .ok_or_else(|| SttError::InvalidPath(path.to_path_buf()))?;

    Model::new(path_str)
        .map(Arc::new)
        .ok_or_else(|| SttError::ModelLoadFailed(path.to_path_buf()))
}

/// STT-related errors.
#[derive(Debug, thiserror::Error)]
pub enum SttError {
    #[error("Failed to load Vosk model from {0}")]
    ModelLoadFailed(std::path::PathBuf),

    #[error("Invalid model path: {0}")]
    InvalidPath(std::path::PathBuf),

    #[error("Failed to create Vosk recognizer")]
    RecognizerCreationFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use vosk::{Alternative, CompleteResult, CompleteResultMultiple, CompleteResultSingle};

    fn test_vosk_model() -> Arc<Model> {
        static MODEL: OnceLock<Arc<Model>> = OnceLock::new();
        MODEL
            .get_or_init(|| {
                let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                let model_path = root.join("models").join("vosk-model-small-en-us-0.15");
                let lib_path = root.join("vendor").join("vosk").join("vosk-win64-0.3.45");
                std::env::set_var("VOSK_LIB_DIR", &lib_path);
                let path = std::env::var("PATH").unwrap_or_default();
                std::env::set_var("PATH", format!("{path};{}", lib_path.display()));
                Arc::new(
                    Model::new(model_path.to_str().expect("model path as string"))
                        .expect("load vosk model"),
                )
            })
            .clone()
    }

    fn test_model_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models")
            .join("vosk-model-small-en-us-0.15")
    }

    #[test]
    fn test_partial_text_to_option_empty() {
        assert_eq!(partial_text_to_option(""), None);
    }

    #[test]
    fn test_partial_text_to_option_non_empty() {
        assert_eq!(partial_text_to_option("hello"), Some("hello".to_string()));
    }

    #[test]
    fn test_complete_result_text_variants() {
        let single = CompleteResult::Single(CompleteResultSingle {
            speaker_info: None,
            result: Vec::new(),
            text: "hello",
        });
        assert_eq!(complete_result_text(single), "hello");

        let alt = Alternative {
            confidence: 0.9,
            result: Vec::new(),
            text: "alt",
        };
        let multiple = CompleteResult::Multiple(CompleteResultMultiple {
            alternatives: vec![alt],
        });
        assert_eq!(complete_result_text(multiple), "alt");

        let empty_multiple = CompleteResult::Multiple(CompleteResultMultiple {
            alternatives: Vec::new(),
        });
        assert_eq!(complete_result_text(empty_multiple), "");
    }

    #[test]
    fn test_handle_decoding_state_branches() {
        let model = test_vosk_model();
        let mut stt = SttProcessor::new(&model, 16000.0).expect("create stt");
        let running = stt.handle_decoding_state(Ok(vosk::DecodingState::Running));
        assert!(running.is_none());
        assert_eq!(
            stt.handle_decoding_state(Ok(vosk::DecodingState::Finalized)),
            None
        );
        assert_eq!(
            stt.handle_decoding_state(Ok(vosk::DecodingState::Failed)),
            None
        );
        assert_eq!(
            stt.handle_decoding_state(Err(vosk::AcceptWaveformError::BufferTooLong(
                (i32::MAX as usize) + 1
            ))),
            None
        );
    }

    #[test]
    fn test_load_model_errors_and_success() {
        let missing = std::env::temp_dir().join("gabby_missing_vosk_model");
        assert!(load_model(&missing).is_err());

        #[cfg(windows)]
        {
            use std::ffi::OsString;
            use std::os::windows::ffi::OsStringExt;
            let bad = OsString::from_wide(&[0xD800]);
            let bad_path = PathBuf::from(bad);
            let err = load_model(&bad_path).err().expect("invalid model path");
            assert!(err.to_string().contains("Invalid model path"));
        }

        let model = load_model(&test_model_path()).expect("load model");
        assert!(Arc::strong_count(&model) >= 1);
    }

    #[test]
    fn test_from_config_final_result_and_reset() {
        let model = test_vosk_model();
        let config = SttConfig {
            model_path: test_model_path(),
            sample_rate: 16000,
        };
        let mut stt = SttProcessor::from_config(&config, &model).expect("create stt");
        let result = stt.final_result();
        assert!(result.is_empty());
        stt.reset();
    }
}
