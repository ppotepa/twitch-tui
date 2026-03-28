# Fork summary

This fork turns `twitch-tui` from a chat-only terminal client into a more streamer-friendly Twitch console with audio, TTS, overlays, multi-channel tabs, diagnostics, and quick actions.

It was built around a real usage workflow: watching streams, switching channels fast, hearing chat and notifications, and using the app while streaming or recording with OBS.

## What this fork adds

### 1. Audio notifications

The app can now play notification sounds for:

- chat messages
- viewer joins
- viewer leaves
- raids

Notifications are configurable in `[notifications]`.

You can choose:

- when they trigger: `all`, `mentions`, `specific`
- what sound to use: `bell`, `beep`, `default`, `file`
- per-event volume
- optional custom sound files

Viewer join/leave tracking is limited to your own channel when desired, because Twitch permissions for chatters are broadcaster/mod scoped.

### 2. Text-to-speech for chat

TTS is now configurable in `[tts]`.

Supported provider chain:

- `edge-tts`
- `espeak-ng`
- `google-cloud`
- `festival`

The fork supports:

- provider fallback chain
- mention/all/specific-user triggers
- skipping your own messages
- skipping bot users
- max message length
- queueing with bounded depth
- optional "only when my stream is live"
- runtime TTS mute toggle

### 3. Boot diagnostics screen

On startup the app now shows a diagnostics screen that checks:

- config load
- token presence
- Twitch auth/login
- required scopes
- audio system
- audio backend routing
- TTS provider availability

This makes it much easier to see why audio or Twitch integrations are not working.

### 4. Viewer join/leave tracking

The fork adds background polling of Twitch chatters and emits:

- `UserJoin`
- `UserLeave`

These events can:

- show messages
- trigger sounds
- be limited to your own channel only

This avoids bad behavior when switching to unrelated channels where your token should not query chatter lists.

### 5. Raid alerts

Raid notifications are detected from Twitch notifications and surfaced in-app, including optional sound playback.

### 6. Clip creation

You can create a Twitch clip directly from the app.

Default key:

- `C` = create clip

The resulting clip URL is surfaced in the UI.

### 7. Highlight log

Messages that mention you can be saved to a persistent highlight log file.

Configured in `[notifications]`:

- `highlight_log_enabled`
- `highlight_log_path`

### 8. Stream status bar

A bottom status bar was added that can show:

- current channel
- live/offline state
- viewer count
- uptime
- game/category
- title
- stream audio volume meter
- TTS mute state

### 9. Live channels overlay

There is now a live channels view that prioritizes channels that are currently online.

Sorting behavior was improved to prefer:

1. live channels
2. higher viewer count
3. alphabetical order

Search behavior was also tightened so obvious substring matches win before fuzzy matches.

### 10. Chat stats overlay

The app now has a chat stats popup showing things like:

- message volume
- top chatters
- top words

### 11. Multi-channel tabs

The fork supports multiple active channel tabs.

Default keys:

- `t` = new tab
- `Tab` = next tab
- `Shift+Tab` = previous tab
- `Ctrl+w` = close tab

The current tab is shown in a tab bar.

### 12. Stream audio controls

The fork adds direct stream audio playback.

Default keys:

- `a` = toggle stream audio
- `V` = open terminal stream viewer

Audio can automatically follow the currently selected channel tab.

### 13. Toast system

System/status messages no longer stay mixed into chat history.

Instead, ephemeral toast notifications are shown and then disappear.

That keeps the chat pane focused on actual stream/chat content.

### 14. Audio routing improvements

The fork adds the base infrastructure for better audio routing:

- stream audio backend selection
- `mpv` or `streamlink`
- output-device config fields
- OBS-oriented routing flags
- audio routing diagnostics on boot

Current supported stream backend selection lives in `[frontend]`.

The config fields for TTS/notification output backend are present and ready, though the main proven path remains the local/default playback route.

## Important default keybinds added by this fork

From `src/config/keybinds.rs`:

- `C` = create clip
- `l` = live channels overlay
- `m` = chat stats overlay
- `a` = audio toggle
- `T` = TTS mute toggle
- `V` = terminal stream viewer
- `t` = new tab
- `Tab` = next tab
- `Shift+Tab` = previous tab
- `Ctrl+w` = close tab

## Main config areas added or extended

### `[frontend]`

Important additions:

- `audio_command`
- `audio_volume`
- `audio_follow_channel_switch`
- `audio_backend`
- `audio_output_device`
- `audio_obs_mode`

### `[notifications]`

Important additions:

- trigger mode
- per-event sound sections
- raid sounds
- chatter polling config
- join/leave messages
- highlight logging
- `chatters_own_channel_only`
- `chatters_channel`
- `output_backend`
- `output_device`

### `[tts]`

Important additions:

- `providers`
- `trigger`
- `trigger_users`
- `max_length`
- `skip_self`
- `only_when_streaming`
- `skip_users`
- `max_queue_depth`
- `output_backend`
- `output_device`
- provider-specific sections:
  - `[tts.edge_tts]`
  - `[tts.espeak_ng]`
  - `[tts.google_cloud]`

## Installation and updating

This fork includes `install.sh`.

What it does:

1. builds the project in release mode
2. looks for `target/release/twt`
3. copies it to `~/.cargo/bin/twt`
4. makes it executable
5. prints the installed version

Usage:

```bash
./install.sh
```

If you prefer a different install location such as `~/.local/bin`, you can either:

- edit `INSTALL_DIR` inside `install.sh`
- or manually copy the built binary

Example manual install:

```bash
cargo build --release
cp target/release/twt ~/.local/bin/twt
chmod +x ~/.local/bin/twt
```

## Current known-good state

At the end of this work, the fork was verified to build and the user confirmed working audio for:

- stream voices/audio
- TTS
- enter/leave notifications

Also completed across the fork:

- 32 tracked todos done
- release build succeeds
- binary runs as `twt`

## Files most changed in this fork

If you want to understand or continue development, start here:

- `src/app.rs`  
  central app loop, audio toggle, tabs, toasts, status bar

- `src/notifications.rs`  
  notification sounds, TTS queue, mute toggle

- `src/audio.rs`  
  shared audio helpers and routing groundwork

- `src/boot.rs`  
  startup diagnostics

- `src/config/frontend.rs`
- `src/config/notifications.rs`
- `src/config/tts.rs`

- `src/ui/components/chat.rs`  
  keybind handling and overlays

- `src/ui/components/chat_stats.rs`
- `src/ui/components/following.rs`

- `src/twitch/chatters_poller.rs`
- `src/twitch/api/streams.rs`
- `src/twitch/api/clips.rs`
- `src/twitch/api/following.rs`

## Practical description of this fork

In plain terms, this fork is:

- a terminal Twitch chat app
- with optional local TTS
- with sound notifications
- with viewer join/leave awareness
- with raid alerts and clip creation
- with live-channel and chat-stats overlays
- with multi-channel tabs
- with stream audio playback
- with better startup diagnostics
- and with groundwork for better OBS/audio routing

## Good next steps if this fork continues

- clean up unused audio-routing scaffolding warnings
- finish full `mpv` backend for TTS/notification output if desired
- improve OBS-specific documentation and presets
- keep `FORK.md` updated whenever behavior changes

