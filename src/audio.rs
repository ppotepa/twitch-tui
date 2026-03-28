use std::{
    fs::File,
    io::{BufReader, Cursor, Write},
    path::Path,
    process::{Child, Command},
    time::Duration,
};

use rodio::{Decoder, OutputStream, Sink, Source, source::SineWave};
use tempfile::NamedTempFile;

use crate::config::AudioOutputBackend;

#[allow(dead_code)]
/// Unified audio routing abstraction for stream audio, TTS, and notifications.
/// Centralizes playback decisions to support:
/// - Different backends (rodio vs mpv)
/// - Different output devices/sinks
/// - OBS-friendly mode (all audio through single path)
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Backend for stream audio playback
    pub stream_backend: AudioBackend,
    /// Backend for TTS and notification playback
    pub output_backend: AudioOutputBackend,
    /// Output device for stream audio (empty = system default)
    pub stream_output_device: String,
    /// Output device for TTS/notifications (empty = system default)
    pub output_device: String,
    /// OBS-friendly mode: routes all audio through unified backend
    pub obs_mode: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioBackend {
    #[default]
    Mpv,
    Streamlink,
}

/// Plays audio through the configured backend.
/// This trait abstracts over rodio Sink and mpv Child process.
#[allow(dead_code)]
pub struct AudioPlayer {
    sink: Option<Sink>,
    _stream: Option<OutputStream>,
}

#[allow(dead_code)]
impl AudioPlayer {
    /// Create a new audio player (rodio-based)
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (stream, stream_handle) = OutputStream::try_default()?;
        let sink = Sink::try_new(&stream_handle)?;
        Ok(Self {
            sink: Some(sink),
            _stream: Some(stream),
        })
    }

    /// Append a sine wave beep to the sink
    pub fn append_beep(&self, volume: f32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.sink.as_ref().map_or_else(
            || Err("No audio sink available".into()),
            |sink| {
                sink.append(
                    SineWave::new(800.0)
                        .take_duration(Duration::from_millis(100))
                        .amplify(volume),
                );
                Ok(())
            },
        )
    }

    /// Append a decoded audio source from bytes
    pub fn append_embedded(
        &self,
        sound_data: &[u8],
        volume: f32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.sink.as_ref().map_or_else(
            || Err("No audio sink available".into()),
            |sink| {
                let source = Decoder::new(Cursor::new(sound_data.to_vec()))?;
                sink.append(source.amplify(volume));
                Ok(())
            },
        )
    }

    /// Append a decoded audio source from file
    pub fn append_file(
        &self,
        path: &Path,
        volume: f32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.sink.as_ref().map_or_else(
            || Err("No audio sink available".into()),
            |sink| {
                let file = File::open(path)?;
                let source = Decoder::new(BufReader::new(file))?;
                sink.append(source.amplify(volume));
                Ok(())
            },
        )
    }

    /// Sleep until all queued audio has finished playing
    pub fn wait(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.sink.as_ref().map_or_else(
            || Err("No audio sink available".into()),
            |sink| {
                sink.sleep_until_end();
                Ok(())
            },
        )
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new().unwrap_or(Self {
            sink: None,
            _stream: None,
        })
    }
}

#[allow(dead_code)]
/// Plays audio through mpv command.
/// Used for OBS capture when mpv backend is selected.
pub fn play_with_mpv(
    audio_source: &str,
    volume: u8,
    extra_args: &[&str],
    output_device: &str,
    client_name: &str,
) -> Result<Child, Box<dyn std::error::Error + Send + Sync>> {
    let volume_arg = format!("--volume={volume}");
    let mut cmd = Command::new("mpv");
    cmd.arg("--no-video").arg(&volume_arg);

    if !client_name.is_empty() {
        cmd.arg(format!("--audio-client-name={client_name}"));
        cmd.env("PULSE_PROP_application.name", client_name);
    }

    if !output_device.is_empty() {
        cmd.arg(format!("--audio-device={output_device}"));
    }

    cmd.env("PULSE_PROP_media.role", "music")
        .args(extra_args)
        .arg(audio_source);

    let child = cmd.spawn()?;
    Ok(child)
}

pub fn apply_mpv_audio_options(args: &mut Vec<String>, output_device: &str, client_name: &str) {
    if !client_name.is_empty()
        && !args
            .iter()
            .any(|arg| arg == "--audio-client-name" || arg.starts_with("--audio-client-name="))
    {
        args.push(format!("--audio-client-name={client_name}"));
    }

    if !output_device.is_empty()
        && !args
            .iter()
            .any(|arg| arg == "--audio-device" || arg.starts_with("--audio-device="))
    {
        args.push(format!("--audio-device={output_device}"));
    }
}

pub fn apply_audio_client_env(cmd: &mut Command, client_name: &str, media_role: &str) {
    if !client_name.is_empty() {
        cmd.env("PULSE_PROP_application.name", client_name);
    }

    if !media_role.is_empty() {
        cmd.env("PULSE_PROP_media.role", media_role);
    }
}

// --- Legacy compatibility helpers (kept for backward compat) ---

pub fn play_beep_blocking(volume: f32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    sink.append(
        SineWave::new(800.0)
            .take_duration(Duration::from_millis(100))
            .amplify(volume),
    );
    sink.sleep_until_end();
    Ok(())
}

pub fn play_embedded_sound_blocking(
    sound_data: &'static [u8],
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    let source = Decoder::new(Cursor::new(sound_data))?;
    sink.append(source.amplify(volume));
    sink.sleep_until_end();
    Ok(())
}

pub fn play_file_blocking(
    path: &Path,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    let file = File::open(path)?;
    let source = Decoder::new(BufReader::new(file))?;
    sink.append(source.amplify(volume));
    sink.sleep_until_end();
    Ok(())
}

pub fn play_file_with_mpv_blocking(
    path: &Path,
    volume: f32,
    output_device: &str,
    client_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let volume = (volume.clamp(0.0, 1.0) * 100.0).round() as u8;
    let mut child = play_with_mpv(
        &path.to_string_lossy(),
        volume,
        &[],
        output_device,
        client_name,
    )?;
    let status = child.wait()?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("mpv exited with status {status}")).into())
    }
}

pub fn play_embedded_sound_with_mpv_blocking(
    sound_data: &'static [u8],
    volume: f32,
    output_device: &str,
    client_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut temp = NamedTempFile::with_suffix(".wav")?;
    temp.write_all(sound_data)?;
    temp.flush()?;

    play_file_with_mpv_blocking(temp.path(), volume, output_device, client_name)
}
