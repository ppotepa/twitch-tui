use async_trait::async_trait;
use base64::Engine;
use serde_json::json;
use tempfile::NamedTempFile;
use tokio::{fs::File, io::AsyncWriteExt, process::Command};
use tracing::warn;

use super::{TtsBackend, TtsProvider};
use crate::{audio::play_file_blocking, config::TtsConfig};

fn shell_escape_single_quotes(text: &str) -> String {
    text.replace('\'', "'\\''")
}

/// Festival TTS provider (local, requires festival installed)
pub struct FestivalProvider;

#[async_trait]
impl TtsBackend for FestivalProvider {
    async fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(format!("echo '{}' | festival --tts", shell_escape_single_quotes(text)));

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Festival TTS failed: {}", stderr);
            return Err(std::io::Error::other(format!("Festival TTS failed: {stderr}")).into());
        }

        Ok(())
    }
}

/// espeak-ng TTS provider (local, lightweight)
pub struct EspeakNgProvider {
    pub voice: String,
    pub speed: i32,
}

#[async_trait]
impl TtsBackend for EspeakNgProvider {
    async fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut cmd = Command::new("espeak-ng");
        cmd.arg("-v")
            .arg(&self.voice)
            .arg("-s").arg(self.speed.to_string())
            .arg(text);

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("espeak-ng TTS failed: {}", stderr);
            return Err(std::io::Error::other(format!("espeak-ng TTS failed: {stderr}")).into());
        }

        Ok(())
    }
}

/// Google Cloud Text-to-Speech provider (REST API, requires GOOGLE_API_KEY env var)
pub struct GoogleCloudProvider {
    pub voice_name: String,
    pub language_code: String,
}

#[async_trait]
impl TtsBackend for GoogleCloudProvider {
    async fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let api_key = std::env::var("GOOGLE_API_KEY").map_err(|_| {
            std::io::Error::other("GOOGLE_API_KEY env var not set")
        })?;

        let url = format!(
            "https://texttospeech.googleapis.com/v1/text:synthesize?key={}",
            api_key
        );

        let body = json!({
            "input": { "text": text },
            "voice": {
                "languageCode": self.language_code,
                "name": self.voice_name
            },
            "audioConfig": { "audioEncoding": "MP3" }
        });

        let client = reqwest::Client::new();
        let response = client.post(&url).json(&body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(std::io::Error::other(
                format!("Google Cloud TTS API error {status}: {body}")
            ).into());
        }

        let json: serde_json::Value = response.json().await?;
        let audio_b64 = json["audioContent"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("Missing audioContent in response"))?;

        let audio_bytes = base64::engine::general_purpose::STANDARD.decode(audio_b64)?;

        // Write to tempfile and play with mpv
        let tmp = NamedTempFile::with_suffix(".mp3")?;
        let tmp_path = tmp.path().to_path_buf();
        let mut f = File::create(&tmp_path).await?;
        f.write_all(&audio_bytes).await?;
        f.flush().await?;
        drop(f);

        tokio::task::spawn_blocking(move || play_file_blocking(&tmp_path, 1.0))
            .await
            .map_err(|err| std::io::Error::other(format!("Google Cloud playback task failed: {err}")))??;

        Ok(())
    }
}

/// edge-tts provider (free Microsoft Edge neural voices, requires `edge-tts` Python package)
pub struct EdgeTtsProvider {
    pub voice: String,
    pub rate: String,
    pub volume: String,
}

#[async_trait]
impl TtsBackend for EdgeTtsProvider {
    async fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let tmp = NamedTempFile::with_suffix(".mp3")?;
        let tmp_path = tmp.path().to_path_buf();

        let synth = Command::new("edge-tts")
            .arg("--voice")
            .arg(&self.voice)
            .arg("--rate")
            .arg(&self.rate)
            .arg("--volume")
            .arg(&self.volume)
            .arg("--text")
            .arg(text)
            .arg("--write-media")
            .arg(&tmp_path)
            .output()
            .await?;

        if !synth.status.success() {
            let stderr = String::from_utf8_lossy(&synth.stderr);
            warn!("edge-tts failed: {}", stderr);
            return Err(std::io::Error::other(format!("edge-tts failed: {stderr}")).into());
        }

        tokio::task::spawn_blocking(move || play_file_blocking(&tmp_path, 1.0))
            .await
            .map_err(|err| std::io::Error::other(format!("edge-tts playback task failed: {err}")))??;

        Ok(())
    }
}

/// Create a TTS provider from config
pub fn create_tts_provider(provider: &TtsProvider, config: &TtsConfig) -> Box<dyn TtsBackend> {
    match provider {
        TtsProvider::Festival => Box::new(FestivalProvider),
        TtsProvider::EspeakNg => Box::new(EspeakNgProvider {
            voice: config
                .voice
                .clone()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| config.espeak_ng.voice.clone()),
            speed: config.rate.unwrap_or(config.espeak_ng.rate),
        }),
        TtsProvider::GoogleCloud => Box::new(GoogleCloudProvider {
            voice_name: config
                .voice
                .clone()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| config.google_cloud.voice.clone()),
            language_code: config
                .language_code
                .clone()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| config.google_cloud.language_code.clone()),
        }),
        TtsProvider::EdgeTts => Box::new(EdgeTtsProvider {
            voice: config
                .voice
                .clone()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| config.edge_tts.voice.clone()),
            rate: config.edge_tts.rate.clone(),
            volume: config.edge_tts.volume.clone(),
        }),
    }
}
