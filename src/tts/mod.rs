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

    /// Speak with optional stereo pan (for 3D spatial mode).
    /// Pan range: -1.0 (left) to 1.0 (right), 0.0 (centre) = no panning.
    /// Default implementation ignores pan and calls `speak()`.
    async fn speak_with_pan(
        &self,
        text: &str,
        _pan: Option<f32>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.speak(text).await
    }
}

pub use providers::create_tts_provider;
