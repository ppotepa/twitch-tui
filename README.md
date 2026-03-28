<div align="center">

# twitch-tui

Twitch chat in the terminal.

[![crates][s1]][l1] [![CI][s2]][l2] [![pre-commit.ci status][s3]][l3] [![unsafe][s4]][l4]

[s1]: https://img.shields.io/crates/v/twitch-tui.svg
[l1]: https://crates.io/crates/twitch-tui
[s2]: https://github.com/Xithrius/twitch-tui/actions/workflows/ci.yml/badge.svg
[l2]: https://github.com/Xithrius/twitch-tui/actions/workflows/ci.yml
[s3]: https://results.pre-commit.ci/badge/github/Xithrius/twitch-tui/main.svg
[l3]: https://results.pre-commit.ci/latest/github/Xithrius/twitch-tui/main
[s4]: https://img.shields.io/badge/unsafe-forbidden-success.svg
[l4]: https://github.com/rust-secure-code/safety-dance/

<img src="assets/preview.png" />

</div>

> **This is a fork** of [Xithrius/twitch-tui](https://github.com/Xithrius/twitch-tui) with streamer-focused additions.
> See [FORK.md](FORK.md) for a full list of what has been added.

## Feature list

- Read/send/search messages
- Switch channels with multi-channel tabs
- Per-channel message history — each tab keeps its own scroll position and history
- Aggregated **All** tab — see every channel's chat in one view with colour-coded channel labels
- Create and toggle filters
- Command, channel, and mention suggestions
- Audio notifications for messages, joins, leaves, and raids
- Text-to-speech for chat messages with configurable provider chain
- Stream audio playback directly from the terminal
- Viewer join/leave tracking and raid alerts
- Live-channel overlay sorted by viewer count
- Chat stats overlay (message volume, top chatters, top words)
- Clip creation from the keyboard
- Stream status bar with live/offline state, viewer count, and uptime
- Boot diagnostics screen — verifies auth, scopes, audio, and TTS on startup
- Highlight log — saves messages that mention you to a file
- Customize functionality and looks using a [config file](https://github.com/Xithrius/twitch-tui/blob/main/default-config.toml)

## Links

- [Documentation](https://xithrius.github.io/twitch-tui/)
- [Setup](https://xithrius.github.io/twitch-tui/guide/installation)

## Installation

```bash
./install.sh
```

Builds a release binary and installs it to `~/.cargo/bin/twt`.

## More information

If you have any problems, do not hesitate to [submit an issue](https://github.com/Xithrius/twitch-tui/issues/new/choose).

Combine this application with [streamlink](https://github.com/streamlink/streamlink) to rid the need of a browser while watching streams.

This project follows the guidelines of [Semantic Versioning](https://semver.org/).

[![](https://raw.githubusercontent.com/dch82/Nixpkgs-Badges/main/nixpkgs-badge-dark.svg)](https://search.nixos.org/packages?size=1&show=twitch-tui)
