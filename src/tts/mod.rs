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

    /// Speak with spatial positioning and optional per-speaker voice selection.
    ///
    /// - `spatial_angle`: position on a circle around the listener in degrees:
    ///   0° = front, 90° = right, 180° = back, 270° = left. `None` = no spatial effect.
    /// - `speaker_hash`: deterministic hash of the username for consistent voice assignment.
    ///
    /// Default implementation ignores both parameters and calls `speak()`.
    async fn speak_with_pan(
        &self,
        text: &str,
        _spatial_angle: Option<f32>,
        _speaker_hash: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.speak(text).await
    }
}

pub use providers::create_tts_provider;
