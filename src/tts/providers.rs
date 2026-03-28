use async_trait::async_trait;
use base64::Engine;
use serde_json::json;
use std::path::Path;
use tempfile::NamedTempFile;
use tokio::{fs::File, io::AsyncWriteExt, process::Command};
use tracing::warn;

use super::{TtsBackend, TtsProvider};
use crate::{
    audio::{play_file_blocking, play_file_with_mpv_blocking},
    config::{AudioOutputBackend, TtsConfig},
};

fn shell_escape_single_quotes(text: &str) -> String {
    text.replace('\'', "'\\''")
}

/// Apply stereo panning to an audio file using ffmpeg.
/// Pan range: -1.0 (full left) to 1.0 (full right), 0.0 (centre) = no panning.
/// Returns the panned audio bytes.
async fn apply_pan_with_ffmpeg(
    input_path: &Path,
    pan: f32,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    // If pan is very close to 0, return the original file
    if pan.abs() < 0.01 {
        return Ok(tokio::fs::read(input_path).await?);
    }

    // Create a temporary output file
    let tmp_out = NamedTempFile::with_suffix(".wav")?;
    let tmp_out_path = tmp_out.path().to_path_buf();

    // ffmpeg pan filter: pan=stereo|c0=L_expr|c1=R_expr
    // We want: pan left => reduce R, pan right => reduce L
    // Clamp pan to [-1, 1]
    let pan_clamped = pan.clamp(-1.0, 1.0);

    // When pan=1 (full right): L channel plays at 0%, R at 100%
    // When pan=-1 (full left): L channel plays at 100%, R at 0%
    // When pan=0 (centre): both at 100%
    let left_level = f32::midpoint(1.0 - pan_clamped, 1.0);
    let right_level = f32::midpoint(1.0, pan_clamped);

    let pan_filter = format!(
        "pan=stereo|c0={left_level}*c0|c1={right_level}*c1"
    );

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-i")
        .arg(input_path)
        .arg("-af")
        .arg(&pan_filter)
        .arg("-y") // Overwrite output file
        .arg(&tmp_out_path);

    let output = cmd.output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("ffmpeg panning failed: {}", stderr);
        return Ok(tokio::fs::read(input_path).await?);
    }

    Ok(tokio::fs::read(&tmp_out_path).await?)
}

/// Festival TTS provider (local, requires festival installed)
pub struct FestivalProvider {
    pub output_client_name: String,
}

#[async_trait]
impl TtsBackend for FestivalProvider {
    async fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(format!(
            "echo '{}' | festival --tts",
            shell_escape_single_quotes(text)
        ));
        if !self.output_client_name.is_empty() {
            cmd.env("PULSE_PROP_application.name", &self.output_client_name)
                .env("PULSE_PROP_media.role", "phone");
        }

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
    pub output_backend: AudioOutputBackend,
    pub output_device: String,
    pub output_client_name: String,
}

#[async_trait]
impl TtsBackend for EspeakNgProvider {
    async fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut cmd = Command::new("espeak-ng");
        cmd.arg("-v")
            .arg(&self.voice)
            .arg("-s")
            .arg(self.speed.to_string());

        match self.output_backend {
            AudioOutputBackend::Rodio => {
                cmd.arg(text);
                if !self.output_client_name.is_empty() {
                    cmd.env("PULSE_PROP_application.name", &self.output_client_name)
                        .env("PULSE_PROP_media.role", "phone");
                }

                let output = cmd.output().await?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("espeak-ng TTS failed: {}", stderr);
                    return Err(
                        std::io::Error::other(format!("espeak-ng TTS failed: {stderr}")).into(),
                    );
                }
            }
            AudioOutputBackend::Mpv => {
                cmd.arg("--stdout").arg(text);
                let output = cmd.output().await?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("espeak-ng TTS failed: {}", stderr);
                    return Err(
                        std::io::Error::other(format!("espeak-ng TTS failed: {stderr}")).into(),
                    );
                }

                let tmp = NamedTempFile::with_suffix(".wav")?;
                let tmp_path = tmp.path().to_path_buf();
                let mut f = File::create(&tmp_path).await?;
                f.write_all(&output.stdout).await?;
                f.flush().await?;
                drop(f);

                let output_device = self.output_device.clone();
                let output_client_name = self.output_client_name.clone();
                tokio::task::spawn_blocking(move || {
                    play_file_with_mpv_blocking(&tmp_path, 1.0, &output_device, &output_client_name)
                })
                .await
                .map_err(|err| {
                    std::io::Error::other(format!("espeak-ng playback task failed: {err}"))
                })??;
            }
        }

        Ok(())
    }

    async fn speak_with_pan(
        &self,
        text: &str,
        pan: Option<f32>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // If no pan or pan is centre-ish, use normal speak
        if pan.is_none_or(|p| p.abs() < 0.01) {
            return self.speak(text).await;
        }

        let pan_value = pan.unwrap_or(0.0);

        let mut cmd = Command::new("espeak-ng");
        cmd.arg("-v")
            .arg(&self.voice)
            .arg("-s")
            .arg(self.speed.to_string())
            .arg("--stdout")
            .arg(text);

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("espeak-ng TTS failed: {}", stderr);
            return Err(
                std::io::Error::other(format!("espeak-ng TTS failed: {stderr}")).into(),
            );
        }

        let tmp = NamedTempFile::with_suffix(".wav")?;
        let tmp_path = tmp.path().to_path_buf();
        let mut f = File::create(&tmp_path).await?;
        f.write_all(&output.stdout).await?;
        f.flush().await?;
        drop(f);

        // Apply panning via ffmpeg
        let panned_audio = apply_pan_with_ffmpeg(&tmp_path, pan_value).await?;

        // Write panned audio to another temp file
        let tmp_panned = NamedTempFile::with_suffix(".wav")?;
        let tmp_panned_path = tmp_panned.path().to_path_buf();
        let mut f_panned = File::create(&tmp_panned_path).await?;
        f_panned.write_all(&panned_audio).await?;
        f_panned.flush().await?;
        drop(f_panned);

        let output_device = self.output_device.clone();
        let output_client_name = self.output_client_name.clone();
        tokio::task::spawn_blocking(move || {
            play_file_with_mpv_blocking(&tmp_panned_path, 1.0, &output_device, &output_client_name)
        })
        .await
        .map_err(|err| {
            std::io::Error::other(format!("espeak-ng playback task failed: {err}"))
        })??;

        Ok(())
    }
}

/// Google Cloud Text-to-Speech provider (REST API, requires `GOOGLE_API_KEY` env var)
pub struct GoogleCloudProvider {
    pub voice_name: String,
    pub language_code: String,
    pub output_backend: AudioOutputBackend,
    pub output_device: String,
    pub output_client_name: String,
}

#[async_trait]
impl TtsBackend for GoogleCloudProvider {
    async fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let api_key = std::env::var("GOOGLE_API_KEY")
            .map_err(|_| std::io::Error::other("GOOGLE_API_KEY env var not set"))?;

        let url = format!("https://texttospeech.googleapis.com/v1/text:synthesize?key={api_key}");

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
            return Err(std::io::Error::other(format!(
                "Google Cloud TTS API error {status}: {body}"
            ))
            .into());
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

        let output_backend = self.output_backend.clone();
        let output_device = self.output_device.clone();
        let output_client_name = self.output_client_name.clone();
        tokio::task::spawn_blocking(move || match output_backend {
            AudioOutputBackend::Rodio => play_file_blocking(&tmp_path, 1.0),
            AudioOutputBackend::Mpv => {
                play_file_with_mpv_blocking(&tmp_path, 1.0, &output_device, &output_client_name)
            }
        })
        .await
        .map_err(|err| {
            std::io::Error::other(format!("Google Cloud playback task failed: {err}"))
        })??;

        Ok(())
    }
}

/// edge-tts provider (free Microsoft Edge neural voices, requires `edge-tts` Python package)
pub struct EdgeTtsProvider {
    pub voice: String,
    pub rate: String,
    pub volume: String,
    pub output_backend: AudioOutputBackend,
    pub output_device: String,
    pub output_client_name: String,
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

        let output_backend = self.output_backend.clone();
        let output_device = self.output_device.clone();
        let output_client_name = self.output_client_name.clone();
        tokio::task::spawn_blocking(move || match output_backend {
            AudioOutputBackend::Rodio => play_file_blocking(&tmp_path, 1.0),
            AudioOutputBackend::Mpv => {
                play_file_with_mpv_blocking(&tmp_path, 1.0, &output_device, &output_client_name)
            }
        })
        .await
        .map_err(|err| std::io::Error::other(format!("edge-tts playback task failed: {err}")))??;

        Ok(())
    }
}

/// Create a TTS provider from config
pub fn create_tts_provider(provider: &TtsProvider, config: &TtsConfig) -> Box<dyn TtsBackend> {
    match provider {
        TtsProvider::Festival => Box::new(FestivalProvider {
            output_client_name: config.output_client_name.clone(),
        }),
        TtsProvider::EspeakNg => Box::new(EspeakNgProvider {
            voice: config
                .voice
                .clone()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| config.espeak_ng.voice.clone()),
            speed: config.rate.unwrap_or(config.espeak_ng.rate),
            output_backend: config.output_backend.clone(),
            output_device: config.output_device.clone(),
            output_client_name: config.output_client_name.clone(),
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
            output_backend: config.output_backend.clone(),
            output_device: config.output_device.clone(),
            output_client_name: config.output_client_name.clone(),
        }),
        TtsProvider::EdgeTts => Box::new(EdgeTtsProvider {
            voice: config
                .voice
                .clone()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| config.edge_tts.voice.clone()),
            rate: config.edge_tts.rate.clone(),
            volume: config.edge_tts.volume.clone(),
            output_backend: config.output_backend.clone(),
            output_device: config.output_device.clone(),
            output_client_name: config.output_client_name.clone(),
        }),
    }
}
