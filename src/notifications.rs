use std::{io::Write, path::Path};

use reqwest::Client;
use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::{
    audio::{
        play_beep_blocking, play_embedded_sound_blocking, play_embedded_sound_with_mpv_blocking,
        play_file_blocking, play_file_with_mpv_blocking,
    },
    config::{AudioOutputBackend, NotificationsConfig, SoundType, TriggerMode, TtsConfig},
    tts::create_tts_provider,
    twitch::api::streams::is_stream_live,
};

// Embed the default notification sounds
static DEFAULT_MESSAGE_SOUND: &[u8] = include_bytes!("../assets/audio/notification-1.wav");
static DEFAULT_JOIN_SOUND: &[u8] = include_bytes!("../assets/enter.wav");
static DEFAULT_LEAVE_SOUND: &[u8] = include_bytes!("../assets/leave.wav");

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum EventType {
    Message,
    UserJoin,
    UserLeave,
}

struct TtsMessage {
    author: String,
    text: String,
}

pub struct NotificationHandler {
    notifications_config: NotificationsConfig,
    tts_config: TtsConfig,
    username: String,
    tts_tx: Option<mpsc::Sender<TtsMessage>>,
    /// Runtime TTS mute toggle (does not change config on disk)
    tts_muted: bool,
}

impl NotificationHandler {
    pub fn new(
        notifications_config: NotificationsConfig,
        tts_config: TtsConfig,
        username: String,
        twitch_client: Option<Client>,
        own_login: String,
    ) -> Self {
        let tts_tx = if tts_config.enabled {
            let (tx, rx) = mpsc::channel::<TtsMessage>(tts_config.max_queue_depth);
            tokio::spawn(tts_worker(rx, tts_config.clone(), twitch_client, own_login));
            Some(tx)
        } else {
            None
        };

        Self {
            notifications_config,
            tts_config,
            username,
            tts_tx,
            tts_muted: false,
        }
    }

    /// Toggle TTS on/off at runtime. Returns the new state (true = muted).
    pub const fn toggle_tts(&mut self) -> bool {
        self.tts_muted = !self.tts_muted;
        self.tts_muted
    }

    pub const fn is_tts_muted(&self) -> bool {
        self.tts_muted
    }

    fn should_trigger(
        &self,
        trigger: &TriggerMode,
        trigger_users: &[String],
        author: &str,
        message: &str,
    ) -> bool {
        match trigger {
            TriggerMode::All => true,
            TriggerMode::Mentions => message
                .split_whitespace()
                .any(|word| word.trim_matches(|c: char| !c.is_alphanumeric()) == self.username),
            TriggerMode::Specific => trigger_users
                .iter()
                .any(|user| user.eq_ignore_ascii_case(author)),
        }
    }

    pub fn play_sound_for_event(&self, event_type: EventType, author: &str, message: &str) {
        if !self.notifications_config.enabled {
            return;
        }

        if !self.should_trigger(
            &self.notifications_config.trigger,
            &self.notifications_config.trigger_users,
            author,
            message,
        ) {
            return;
        }

        let event_sounds = match event_type {
            EventType::Message => &self.notifications_config.messages,
            EventType::UserJoin => &self.notifications_config.joins,
            EventType::UserLeave => &self.notifications_config.leaves,
        };

        if !event_sounds.enabled {
            return;
        }

        match event_sounds.sound_type {
            SoundType::Bell => {
                print!("\x07");
                let _ = std::io::stdout().flush();
            }
            SoundType::Beep => {
                let volume = event_sounds.volume;
                let config = self.notifications_config.clone();
                tokio::spawn(play_beep(config, volume));
            }
            SoundType::Default => {
                let volume = event_sounds.volume;
                let config = self.notifications_config.clone();
                tokio::spawn(play_default_sound_for_event(config, event_type, volume));
            }
            SoundType::File => {
                if let Some(ref sound_file) = event_sounds.sound_file {
                    let volume = event_sounds.volume;
                    let config = self.notifications_config.clone();
                    tokio::spawn(play_sound_file(config, sound_file.clone(), volume));
                }
            }
        }
    }

    pub fn play_sound(&self, author: &str, message: &str) {
        self.play_sound_for_event(EventType::Message, author, message);
    }

    pub fn speak(&self, author: &str, message: &str) {
        if self.tts_muted {
            return;
        }

        let Some(ref tts_tx) = self.tts_tx else {
            return;
        };

        if !self.should_trigger(
            &self.tts_config.trigger,
            &self.tts_config.trigger_users,
            author,
            message,
        ) {
            return;
        }

        // Skip own messages
        if self.tts_config.skip_self && author.eq_ignore_ascii_case(&self.username) {
            return;
        }

        // Skip bot users
        if self
            .tts_config
            .skip_users
            .iter()
            .any(|u| u.eq_ignore_ascii_case(author))
        {
            return;
        }

        let text = if message.len() > self.tts_config.max_length {
            format!("{}...", &message[..self.tts_config.max_length])
        } else {
            message.to_string()
        };

        // try_send: drops message silently if queue is full (oldest retained, newest dropped)
        let _ = tts_tx.try_send(TtsMessage {
            author: author.to_string(),
            text,
        });
    }
}

/// Background worker — drains the TTS queue one message at a time.
async fn tts_worker(
    mut rx: mpsc::Receiver<TtsMessage>,
    config: TtsConfig,
    client: Option<Client>,
    own_login: String,
) {
    while let Some(msg) = rx.recv().await {
        // Only speak when own stream is live
        if config.only_when_streaming {
            let live = if let Some(ref c) = client {
                is_stream_live(c, &own_login).await
            } else {
                false
            };
            if !live {
                continue;
            }
        }

        if let Err(e) = speak_text(&config, &msg.author, &msg.text).await {
            error!("TTS error: {}", e);
        }
    }
}

async fn play_beep(config: NotificationsConfig, volume: f32) {
    tokio::task::spawn_blocking(move || {
        let result = match config.output_backend {
            AudioOutputBackend::Rodio | AudioOutputBackend::Mpv => play_beep_blocking(volume),
        };

        if let Err(e) = result {
            warn!("Failed to play beep: {}", e);
        }
    })
    .await
    .ok();
}

async fn play_default_sound_for_event(
    config: NotificationsConfig,
    event_type: EventType,
    volume: f32,
) {
    tokio::task::spawn_blocking(move || {
        if let Err(e) = play_default_sound_for_event_blocking(&config, event_type, volume) {
            warn!("Failed to play default sound: {}", e);
        }
    })
    .await
    .ok();
}

fn play_default_sound_for_event_blocking(
    config: &NotificationsConfig,
    event_type: EventType,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let sound_data = match event_type {
        EventType::Message => DEFAULT_MESSAGE_SOUND,
        EventType::UserJoin => DEFAULT_JOIN_SOUND,
        EventType::UserLeave => DEFAULT_LEAVE_SOUND,
    };

    match config.output_backend {
        AudioOutputBackend::Rodio => play_embedded_sound_blocking(sound_data, volume),
        AudioOutputBackend::Mpv => play_embedded_sound_with_mpv_blocking(
            sound_data,
            volume,
            &config.output_device,
            &config.output_client_name,
        ),
    }
}

async fn play_sound_file(config: NotificationsConfig, path: String, volume: f32) {
    tokio::task::spawn_blocking(move || {
        if let Err(e) = play_sound_file_blocking(&config, &path, volume) {
            warn!("Failed to play sound file '{}': {}", path, e);
        }
    })
    .await
    .ok();
}

fn play_sound_file_blocking(
    config: &NotificationsConfig,
    path: &str,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !Path::new(path).exists() {
        return Err(format!("Sound file not found: {path}").into());
    }

    match config.output_backend {
        AudioOutputBackend::Rodio => play_file_blocking(Path::new(path), volume),
        AudioOutputBackend::Mpv => play_file_with_mpv_blocking(
            Path::new(path),
            volume,
            &config.output_device,
            &config.output_client_name,
        ),
    }
}

async fn speak_text(
    config: &TtsConfig,
    author: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let formatted_text = format!("{author} says: {text}");
    let mut last_error: Option<Box<dyn std::error::Error + Send + Sync>> = None;

    for provider in config.ordered_providers() {
        match create_tts_provider(&provider, config)
            .speak(&formatted_text)
            .await
        {
            Ok(()) => return Ok(()),
            Err(err) => {
                warn!("TTS provider {:?} failed, trying fallback", provider);
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| std::io::Error::other("No TTS providers configured").into()))
}
