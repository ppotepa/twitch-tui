use serde::{Deserialize, Serialize};

use super::notifications::{AudioOutputBackend, TriggerMode};
use crate::tts::TtsProvider;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct EspeakNgTtsConfig {
    pub voice: String,
    pub rate: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct GoogleCloudTtsConfig {
    pub voice: String,
    pub language_code: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct EdgeTtsTtsConfig {
    pub voice: String,
    /// Speech rate adjustment, e.g. "-10%" to slow down, "+10%" to speed up
    pub rate: String,
    /// Volume adjustment, e.g. "+0%" (no change), "-20%" to reduce
    pub volume: String,
}

impl Default for EdgeTtsTtsConfig {
    fn default() -> Self {
        Self {
            voice: "en-US-JennyNeural".to_string(),
            rate: "-10%".to_string(),
            volume: "+0%".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct TtsConfig {
    /// Enable text-to-speech
    pub enabled: bool,
    /// Ordered TTS providers to try
    pub providers: Vec<TtsProvider>,
    /// Legacy single-provider setting kept for backwards compatibility
    pub provider: Option<TtsProvider>,
    /// Legacy shared voice setting
    pub voice: Option<String>,
    /// Legacy shared speech rate setting
    pub rate: Option<i32>,
    /// Legacy shared Google Cloud language code
    pub language_code: Option<String>,
    /// When to trigger TTS
    pub trigger: TriggerMode,
    /// Specific usernames to trigger on (only used if trigger is "specific")
    pub trigger_users: Vec<String>,
    /// Maximum message length to speak (prevents very long messages)
    pub max_length: usize,
    /// Skip messages sent by yourself
    pub skip_self: bool,
    /// Only speak when your own stream is live
    pub only_when_streaming: bool,
    /// Usernames whose messages are never spoken (bots etc.)
    pub skip_users: Vec<String>,
    /// Maximum number of messages queued for TTS (oldest dropped when full)
    pub max_queue_depth: usize,
    /// Audio backend for TTS playback (rodio or mpv)
    pub output_backend: AudioOutputBackend,
    /// Output device/sink for TTS (empty = system default)
    pub output_device: String,
    /// espeak-ng provider settings
    pub espeak_ng: EspeakNgTtsConfig,
    /// google-cloud provider settings
    pub google_cloud: GoogleCloudTtsConfig,
    /// edge-tts provider settings
    pub edge_tts: EdgeTtsTtsConfig,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            providers: vec![TtsProvider::default()],
            provider: None,
            voice: None,
            rate: None,
            language_code: None,
            trigger: TriggerMode::default(),
            trigger_users: Vec::new(),
            max_length: 200,
            skip_self: true,
            only_when_streaming: false,
            skip_users: vec![
                "streamelements".into(),
                "nightbot".into(),
                "moobot".into(),
                "fossabot".into(),
                "streamlabs".into(),
                "soundalerts".into(),
            ],
            max_queue_depth: 3,
            output_backend: AudioOutputBackend::default(),
            output_device: String::new(),
            espeak_ng: EspeakNgTtsConfig {
                voice: "en-us".to_string(),
                rate: 175,
            },
            google_cloud: GoogleCloudTtsConfig {
                voice: "en-US-Wavenet-F".to_string(),
                language_code: "en-US".to_string(),
            },
            edge_tts: EdgeTtsTtsConfig::default(),
        }
    }
}

impl TtsConfig {
    #[allow(dead_code)]
    pub fn is_configured(&self) -> bool {
        self.enabled
    }

    pub fn ordered_providers(&self) -> Vec<TtsProvider> {
        if !self.providers.is_empty() {
            self.providers.clone()
        } else if let Some(provider) = &self.provider {
            vec![provider.clone()]
        } else {
            vec![TtsProvider::default()]
        }
    }
}
