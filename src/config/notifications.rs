use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AudioOutputBackend {
    /// Use rodio for local playback (default)
    Rodio,
    /// Use mpv for playback (better for OBS capture)
    Mpv,
}

impl Default for AudioOutputBackend {
    fn default() -> Self {
        Self::Rodio
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TriggerMode {
    /// Trigger on all messages
    All,
    /// Only trigger when username is mentioned
    Mentions,
    /// Only trigger for specific usernames
    Specific,
}

impl Default for TriggerMode {
    fn default() -> Self {
        Self::Mentions
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SoundType {
    /// Use terminal bell character
    Bell,
    /// Play generated beep sound (rodio)
    Beep,
    /// Play default embedded sound file
    Default,
    /// Play custom sound file
    File,
}

impl Default for SoundType {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct EventSounds {
    /// Enable sound for this event type
    pub enabled: bool,
    /// Type of sound to play
    pub sound_type: SoundType,
    /// Path to custom sound file (only used if sound_type is "file")
    pub sound_file: Option<String>,
    /// Volume level (0.0 to 1.0)
    pub volume: f32,
}

impl Default for EventSounds {
    fn default() -> Self {
        Self {
            enabled: false,
            sound_type: SoundType::Default,
            sound_file: None,
            volume: 0.5,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct NotificationsConfig {
    /// Enable sound notifications
    pub enabled: bool,
    /// When to trigger notifications
    pub trigger: TriggerMode,
    /// Specific usernames to trigger on (only used if trigger is "specific")
    pub trigger_users: Vec<String>,
    /// Chat message notifications
    pub messages: EventSounds,
    /// User join notifications
    pub joins: EventSounds,
    /// User leave notifications
    pub leaves: EventSounds,
    /// Raid notifications
    pub raids: EventSounds,
    /// Polling interval in seconds for fetching current viewers (requires moderator:read:chatters scope)
    pub viewer_poll_interval_secs: u64,
    /// Message shown in chat when a viewer joins. Use `{user}` as placeholder.
    pub join_message: String,
    /// Message shown in chat when a viewer leaves. Use `{user}` as placeholder.
    pub leave_message: String,
    /// Enable saving @mention highlights to a log file
    pub highlight_log_enabled: bool,
    /// Path to the highlights log file
    pub highlight_log_path: String,
    /// Only track viewer join/leave for your own channel (recommended — requires broadcaster/moderator scope)
    pub chatters_own_channel_only: bool,
    /// Your channel name for chatters tracking (leave empty to use twitch.channel)
    pub chatters_channel: String,
    /// Audio backend for notification sounds (rodio or mpv)
    pub output_backend: AudioOutputBackend,
    /// Output device/sink for notifications (empty = system default)
    pub output_device: String,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger: TriggerMode::default(),
            trigger_users: Vec::new(),
            messages: EventSounds {
                enabled: true,
                sound_type: SoundType::Default,
                sound_file: None,
                volume: 0.5,
            },
            joins: EventSounds {
                enabled: true,
                sound_type: SoundType::Default,
                sound_file: None,
                volume: 0.3,
            },
            leaves: EventSounds {
                enabled: true,
                sound_type: SoundType::Default,
                sound_file: None,
                volume: 0.3,
            },
            raids: EventSounds {
                enabled: true,
                sound_type: SoundType::Default,
                sound_file: None,
                volume: 0.8,
            },
            viewer_poll_interval_secs: 30,
            join_message: "👋 @{user} joined".to_string(),
            leave_message: "👋 @{user} left".to_string(),
            highlight_log_enabled: false,
            highlight_log_path: directories::BaseDirs::new()
                .map(|b| {
                    b.data_local_dir()
                        .join("twt")
                        .join("highlights.log")
                        .to_string_lossy()
                        .into_owned()
                })
                .unwrap_or_else(|| "~/.local/share/twt/highlights.log".to_string()),
            chatters_own_channel_only: true,
            chatters_channel: String::new(),
            output_backend: AudioOutputBackend::default(),
            output_device: String::new(),
        }
    }
}
