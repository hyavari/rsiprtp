//! Audio processing pipeline components.
//!
//! This module contains the STT, LLM, TTS, and VAD components that form
//! the audio processing pipeline for voice conversations.

pub mod llm;
pub mod stt;
pub mod tts;
pub mod vad;

pub use llm::OllamaClient;
pub use stt::SttProcessor;
pub use tts::TtsProcessor;
pub use vad::{VadDecision, VadState};
