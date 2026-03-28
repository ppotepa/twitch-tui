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

/// Curated English edge-tts voices covering diverse accents and both genders.
/// Indexed deterministically by `speaker_hash % len` to give each chatter a fixed voice.
const EDGE_TTS_SPATIAL_VOICES: &[&str] = &[
    "en-US-AndrewNeural",
    "en-US-AriaNeural",
    "en-US-AvaNeural",
    "en-US-BrianNeural",
    "en-US-ChristopherNeural",
    "en-US-EmmaNeural",
    "en-US-EricNeural",
    "en-US-GuyNeural",
    "en-US-JennyNeural",
    "en-US-MichelleNeural",
    "en-US-RogerNeural",
    "en-US-SteffanNeural",
    "en-GB-LibbyNeural",
    "en-GB-MaisieNeural",
    "en-GB-RyanNeural",
    "en-GB-SoniaNeural",
    "en-GB-ThomasNeural",
    "en-AU-NatashaNeural",
    "en-AU-WilliamMultilingualNeural",
    "en-CA-ClaraNeural",
    "en-CA-LiamNeural",
    "en-IE-ConnorNeural",
    "en-IE-EmilyNeural",
    "en-IN-NeerjaNeural",
    "en-IN-PrabhatNeural",
    "en-NZ-MitchellNeural",
    "en-NZ-MollyNeural",
    "en-ZA-LeahNeural",
    "en-ZA-LukeNeural",
];

/// Curated espeak-ng voice variants (language+variant) for spatial mode.
const ESPEAK_SPATIAL_VOICES: &[&str] = &[
    "en+m1", "en+m2", "en+m3", "en+m4", "en+m5", "en+m6", "en+m7", "en+f1", "en+f2", "en+f3",
    "en+f4", "en+f5", "en-us+m1", "en-us+m2", "en-us+m3", "en-us+f1", "en-us+f2", "en-us+f3",
    "en-gb+m1", "en-gb+m2", "en-gb+m3", "en-gb+f1", "en-gb+f2",
];

/// Build an ffmpeg audio filter string for full 3D spatial positioning.
/// `angle_deg`: degrees around the listener (0=front, 90=right, 180=back, 270=left).
///
/// Encodes three psychoacoustic cues:
///   1. ILD (Interaural Level Difference): level attenuation on recessive ear
///   2. ITD (Interaural Time Delay): up to 0.65ms delay on the far ear -- primary cue
///   3. Front/back pinna simulation: rear sources get lowpass + volume reduction
///
/// Input is assumed mono (TTS); both output channels source from c0.
fn build_spatial_filter(angle_deg: f32) -> String {
    let angle_rad = angle_deg.to_radians();
    let pan = angle_rad.sin(); // azimuth: -1=left, 0=front/back, +1=right
    let cos_val = angle_rad.cos(); // +1=front, -1=back

    // ILD: level attenuation
    let left = 1.0_f32 - pan.max(0.0);
    let right = 1.0_f32 + pan.min(0.0);
    let pan_filter = format!("pan=stereo|c0={left:.4}*c0|c1={right:.4}*c0");

    // ITD: time delay up to 0.65ms at 90 degrees - primary cue for lateral localisation
    // Source on right: left ear hears it later, so delay left channel.
    // Source on left:  right ear hears it later, so delay right channel.
    let max_itd_ms = 0.65_f32;
    let (left_delay_ms, right_delay_ms) = if pan > 0.0 {
        (pan * max_itd_ms, 0.0_f32)
    } else {
        (0.0_f32, (-pan) * max_itd_ms)
    };
    let itd_filter = format!("adelay={left_delay_ms:.3}|{right_delay_ms:.3}");

    // Front/back: pinna occlusion for rear sources
    let back_depth = (-cos_val).max(0.0); // 0 at front, 1 directly behind
    let mut parts: Vec<String> = if back_depth > 0.05 {
        let cutoff = (8000.0 - back_depth * 4000.0) as u32;
        let volume = 1.0 - back_depth * 0.25;
        vec![format!("lowpass=f={cutoff}"), format!("volume={volume:.4}")]
    } else {
        vec![]
    };

    // Filter order: [rear EQ on mono source] -> pan (mono->stereo) -> ITD delay
    parts.push(pan_filter);
    parts.push(itd_filter);
    parts.join(",")
}

/// Apply full spatial audio to a file using ffmpeg.
/// `angle_deg` is the source position on a circle around the listener:
///   0° = front, 90° = right, 180° = back, 270° = left.
/// Input can be any format ffmpeg understands. Output is always WAV.
async fn apply_spatial_with_ffmpeg(
    input_path: &Path,
    angle_deg: f32,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let filter = build_spatial_filter(angle_deg);

    let tmp_out = NamedTempFile::with_suffix(".wav")?;
    let tmp_out_path = tmp_out.path().to_path_buf();

    let out = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-af")
        .arg(&filter)
        .arg(&tmp_out_path)
        .output()
        .await?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        warn!("ffmpeg spatial filter failed: {}", stderr);
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
    pub playback_volume: f32,
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
        spatial_angle: Option<f32>,
        speaker_hash: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if spatial_angle.is_none() {
            return self.speak(text).await;
        }
        let angle = spatial_angle.unwrap_or(0.0);

        // Pick a voice variant deterministically from the speaker hash
        let voice = speaker_hash.map_or(self.voice.as_str(), |hash| {
            ESPEAK_SPATIAL_VOICES[(hash as usize) % ESPEAK_SPATIAL_VOICES.len()]
        });

        let mut cmd = Command::new("espeak-ng");
        cmd.arg("-v")
            .arg(voice)
            .arg("-s")
            .arg(self.speed.to_string())
            .arg("--stdout")
            .arg(text);

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("espeak-ng TTS failed: {}", stderr);
            return Err(std::io::Error::other(format!("espeak-ng TTS failed: {stderr}")).into());
        }

        let tmp = NamedTempFile::with_suffix(".wav")?;
        let tmp_path = tmp.path().to_path_buf();
        let mut f = File::create(&tmp_path).await?;
        f.write_all(&output.stdout).await?;
        f.flush().await?;
        drop(f);

        // Apply spatial filter (pan + front/back EQ) via ffmpeg
        let spatial_audio = apply_spatial_with_ffmpeg(&tmp_path, angle).await?;

        let tmp_spatial = NamedTempFile::with_suffix(".wav")?;
        let tmp_spatial_path = tmp_spatial.path().to_path_buf();
        let mut f_out = File::create(&tmp_spatial_path).await?;
        f_out.write_all(&spatial_audio).await?;
        f_out.flush().await?;
        drop(f_out);

        let output_backend = self.output_backend.clone();
        let output_device = self.output_device.clone();
        let output_client_name = self.output_client_name.clone();
        let playback_volume = self.playback_volume;
        tokio::task::spawn_blocking(move || match output_backend {
            AudioOutputBackend::Rodio => play_file_blocking(&tmp_spatial_path, playback_volume),
            AudioOutputBackend::Mpv => play_file_with_mpv_blocking(
                &tmp_spatial_path,
                playback_volume,
                &output_device,
                &output_client_name,
            ),
        })
        .await
        .map_err(|err| std::io::Error::other(format!("espeak-ng playback task failed: {err}")))??;

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
    pub playback_volume: f32,
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
        let playback_volume = self.playback_volume;
        tokio::task::spawn_blocking(move || match output_backend {
            AudioOutputBackend::Rodio => play_file_blocking(&tmp_path, playback_volume),
            AudioOutputBackend::Mpv => {
                play_file_with_mpv_blocking(&tmp_path, playback_volume, &output_device, &output_client_name)
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
    pub playback_volume: f32,
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
        let playback_volume = self.playback_volume;
        tokio::task::spawn_blocking(move || match output_backend {
            AudioOutputBackend::Rodio => play_file_blocking(&tmp_path, playback_volume),
            AudioOutputBackend::Mpv => {
                play_file_with_mpv_blocking(&tmp_path, playback_volume, &output_device, &output_client_name)
            }
        })
        .await
        .map_err(|err| std::io::Error::other(format!("edge-tts playback task failed: {err}")))??;

        Ok(())
    }

    async fn speak_with_pan(
        &self,
        text: &str,
        spatial_angle: Option<f32>,
        speaker_hash: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if spatial_angle.is_none() {
            return self.speak(text).await;
        }
        let angle = spatial_angle.unwrap_or(0.0);

        // Pick a voice deterministically from the speaker hash
        let voice = speaker_hash.map_or(self.voice.as_str(), |hash| {
            EDGE_TTS_SPATIAL_VOICES[(hash as usize) % EDGE_TTS_SPATIAL_VOICES.len()]
        });

        // Step 1: generate TTS audio to mp3 via edge-tts
        let tmp = NamedTempFile::with_suffix(".mp3")?;
        let tmp_path = tmp.path().to_path_buf();

        let synth = Command::new("edge-tts")
            .arg("--voice")
            .arg(voice)
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

        // Step 2: apply spatial filter (pan + front/back EQ) via ffmpeg
        let spatial_audio = apply_spatial_with_ffmpeg(&tmp_path, angle).await?;

        let tmp_spatial = NamedTempFile::with_suffix(".wav")?;
        let tmp_spatial_path = tmp_spatial.path().to_path_buf();
        let mut f = File::create(&tmp_spatial_path).await?;
        f.write_all(&spatial_audio).await?;
        f.flush().await?;
        drop(f);

        // Step 3: play the spatialised WAV
        let output_backend = self.output_backend.clone();
        let output_device = self.output_device.clone();
        let output_client_name = self.output_client_name.clone();
        let playback_volume = self.playback_volume;
        tokio::task::spawn_blocking(move || match output_backend {
            AudioOutputBackend::Rodio => play_file_blocking(&tmp_spatial_path, playback_volume),
            AudioOutputBackend::Mpv => play_file_with_mpv_blocking(
                &tmp_spatial_path,
                playback_volume,
                &output_device,
                &output_client_name,
            ),
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
            playback_volume: config.volume as f32 / 100.0,
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
            playback_volume: config.volume as f32 / 100.0,
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
            playback_volume: config.volume as f32 / 100.0,
        }),
    }
}
