# Sound Notifications & TTS Features

This fork adds comprehensive sound notifications and text-to-speech (TTS) capabilities to twitch-tui.

## Features

### 1. Multi-Event Sound System
- **Chat Messages**: Custom notification sound for regular chat messages
- **User Events**: Separate sounds for join/leave events (when supported)
- **Embedded Sounds**: Three built-in high-quality notification sounds
- **Custom Sounds**: Support for your own .wav, .ogg, or .mp3 files

### 2. Sound Types
- **Terminal Bell**: System beep (zero dependencies)
- **Generated Beep**: Clean 800Hz tone via rodio
- **Default Embedded**: Built-in notification sounds (3 different sounds)
- **Custom Files**: Your own sound files

### 3. Text-to-Speech (TTS)
- Shell out to external TTS engines (Festival, espeak-ng, macOS `say`)
- Configurable voice, rate, volume, and language
- Automatic message truncation for long messages

### 4. Flexible Triggering
All features support three trigger modes:
- **All**: Trigger on every chat message
- **Mentions**: Only when your username is mentioned  
- **Specific**: Only for specific usernames

## Configuration

Add these sections to your `config.toml`:

### Multi-Event Sound Notifications

```toml
[notifications]
# Global notification settings
enabled = true
trigger = "all"  # or "mentions" or "specific"
trigger_users = ["streamer", "mod1"]

# Chat message sounds
[notifications.messages]
enabled = true
sound_type = "default"  # Built-in notification sound
sound_file = ""
volume = 0.5

# User join sounds  
[notifications.joins]
enabled = true
sound_type = "default"  # Built-in join sound
sound_file = ""
volume = 0.3

# User leave sounds
[notifications.leaves]  
enabled = true
sound_type = "default"  # Built-in leave sound
sound_file = ""
volume = 0.3
```

### Text-to-Speech

```toml
[tts]
# Enable TTS
enabled = false
# TTS command (e.g., "espeak-ng", "piper", "say")
command = "espeak-ng"
# Arguments - use {text} as placeholder for message
args = ["-v", "en-us", "{text}"]
# Optional: voice identifier
voice = "en-us"
# Optional: speech rate (80-400)
rate = 175
# Optional: volume (0-100)
volume = 50
# Trigger: "all", "mentions", or "specific"
trigger = "mentions"
# Specific users (if trigger = "specific")
trigger_users = []
# Max message length to speak
max_length = 200
```

## TTS Engine Examples

### Linux: Festival (Recommended - Natural Speech)
```toml
command = "bash"
args = ["-c", "echo '{text}' | festival --tts"]
```

### Linux: espeak-ng (Lightweight)
```toml
command = "espeak-ng"
args = ["-v", "en-us", "-s", "160", "{text}"]
```

### macOS: say
```toml
command = "say"
args = ["-v", "Alex", "-r", "175", "{text}"]
```

## Installation

### Quick Install
```bash
./install.sh
```

This builds the release binary and copies it to `~/.cargo/bin/twt`.

### Manual Install
```bash
cargo build --release
cp target/release/twt ~/.cargo/bin/
```

### Or Use cargo install
```bash
cargo install --path .
```

## Dependencies

- **rodio**: Audio playback for custom sound files
- **tokio (process feature)**: For spawning TTS commands

## Usage Examples

### Example 1: Bell on Mentions
```toml
[notifications]
enabled = true
sound_type = "bell"
trigger = "mentions"
```

### Example 2: Custom Sound for Specific Users
```toml
[notifications]
enabled = true
sound_type = "file"
sound_file = "/home/user/sounds/ping.wav"
trigger = "specific"
trigger_users = ["important_person", "moderator"]
```

### Example 3: TTS for All Messages
```toml
[tts]
enabled = true
command = "espeak-ng"
args = ["-v", "en-us", "{text}"]
trigger = "all"
max_length = 100
```

### Example 4: Combined - Sound + TTS on Mentions
```toml
[notifications]
enabled = true
sound_type = "bell"
trigger = "mentions"

[tts]
enabled = true
command = "say"
args = ["{text}"]
trigger = "mentions"
max_length = 200
```

## Troubleshooting

### No sound playing
- Check if your terminal supports BEL character (most do)
- For file playback, ensure the sound file path is correct
- Check audio output devices are working

### TTS not working
- Ensure the TTS command is installed (`which espeak-ng`)
- Test the command manually: `espeak-ng "test message"`
- Check logs for error messages

### High CPU usage
- Reduce TTS max_length
- Use trigger = "mentions" instead of "all"
- Disable features when not needed

## Performance Notes

- Sound notifications have minimal overhead (especially bell)
- TTS spawns async subprocesses - no blocking
- File playback is offloaded to background threads

## License

Same as upstream: MIT OR Apache-2.0
