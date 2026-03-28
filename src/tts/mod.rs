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
}

pub use providers::create_tts_provider;
