pub mod providers;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub enum TtsProvider {
    #[serde(rename = "festival")]
    Festival,
    #[serde(rename = "espeak-ng")]
    EspeakNg,
    #[serde(rename = "google-cloud")]
    GoogleCloud,
    #[serde(rename = "edge-tts")]
    #[default]
    EdgeTts,
}

/// TTS provider trait for pluggable speech synthesis
#[async_trait]
pub trait TtsBackend: Send + Sync {
    async fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Speak with optional stereo pan and optional per-speaker voice selection.
    ///
    /// - `pan`: -1.0 (full left) to 1.0 (full right), `None` = centre
    /// - `speaker_hash`: deterministic hash of the username; providers map this
    ///   to a voice in their own format so every chatter has a unique voice.
    ///
    /// Default implementation ignores both parameters and calls `speak()`.
    async fn speak_with_pan(
        &self,
        text: &str,
        _pan: Option<f32>,
        _speaker_hash: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.speak(text).await
    }
}

pub use providers::create_tts_provider;
