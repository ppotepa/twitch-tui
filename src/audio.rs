use std::{
    fs::File,
    io::{BufReader, Cursor},
    path::Path,
    process::{Child, Command},
    time::Duration,
};

use rodio::{Decoder, OutputStream, Sink, Source, source::SineWave};

use crate::config::AudioOutputBackend;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBackend {
    Mpv,
    Streamlink,
}

impl Default for AudioBackend {
    fn default() -> Self {
        Self::Mpv
    }
}

/// Plays audio through the configured backend.
/// This trait abstracts over rodio Sink and mpv Child process.
pub struct AudioPlayer {
    sink: Option<Sink>,
    _stream: Option<OutputStream>,
}

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
    pub fn append_beep(&mut self, volume: f32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref sink) = self.sink {
            sink.append(
                SineWave::new(800.0)
                    .take_duration(Duration::from_millis(100))
                    .amplify(volume),
            );
            Ok(())
        } else {
            Err("No audio sink available".into())
        }
    }

    /// Append a decoded audio source from bytes
    pub fn append_embedded(
        &mut self,
        sound_data: &[u8],
        volume: f32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref sink) = self.sink {
            let source = Decoder::new(Cursor::new(sound_data.to_vec()))?;
            sink.append(source.amplify(volume));
            Ok(())
        } else {
            Err("No audio sink available".into())
        }
    }

    /// Append a decoded audio source from file
    pub fn append_file(
        &mut self,
        path: &Path,
        volume: f32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref sink) = self.sink {
            let file = File::open(path)?;
            let source = Decoder::new(BufReader::new(file))?;
            sink.append(source.amplify(volume));
            Ok(())
        } else {
            Err("No audio sink available".into())
        }
    }

    /// Sleep until all queued audio has finished playing
    pub fn wait(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref sink) = self.sink {
            sink.sleep_until_end();
            Ok(())
        } else {
            Err("No audio sink available".into())
        }
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

/// Plays audio through mpv command.
/// Used for OBS capture when mpv backend is selected.
pub fn play_with_mpv(
    audio_source: &str,
    volume: u8,
    extra_args: &[&str],
) -> Result<Child, Box<dyn std::error::Error + Send + Sync>> {
    let volume_arg = format!("--volume={}", volume);
    let mut cmd = Command::new("mpv");
    cmd.arg("--no-video")
        .arg(&volume_arg)
        .args(extra_args)
        .arg(audio_source);

    let child = cmd.spawn()?;
    Ok(child)
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
