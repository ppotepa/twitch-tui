use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    fmt::Write as _,
    io::{Read, Write, stdout},
    process::{Child, Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

use color_eyre::Result;
use tokio::sync::{
    mpsc::{Receiver, Sender},
    oneshot::Receiver as OSReceiver,
};
use tracing::{error, warn};
use tui::{
    DefaultTerminal, Frame,
    crossterm::{
        cursor::EnableBlinking,
        event::{DisableMouseCapture, EnableMouseCapture},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::{
    audio::{apply_audio_client_env, apply_mpv_audio_options},
    config::{AudioBackend, SharedCoreConfig},
    emotes::{
        ApplyCommand, DecodedEmote, DownloadedEmotes, Emotes, SharedEmotes, display_emote,
        query_emotes,
    },
    events::{
        Event, Events, InternalEvent, StreamStatusInfo, TwitchAction, TwitchEvent,
        TwitchNotification,
    },
    handlers::{
        data::{DataBuilder, KNOWN_CHATTERS, MessageData},
        filters::Filters,
        state::State,
        storage::{SharedStorage, Storage},
    },
    notifications::{EventType, NotificationHandler},
    twitch::{
        api::{channels::get_channel_id, clips::create_clip, streams::get_stream_info},
        oauth::TwitchOauth,
    },
    ui::components::{Component, Components},
    utils::sanitization::clean_channel_name,
};

pub type SharedMessages = Rc<RefCell<VecDeque<MessageData>>>;

struct ToastMessage {
    text: String,
    expires_at: Instant,
}

pub struct App {
    pub running: bool,

    /// UI components
    pub components: Components,

    /// Configuration loaded from file and CLI arguments
    pub config: SharedCoreConfig,

    /// Twitch OAuth client and session info
    pub twitch_oauth: TwitchOauth,
    pub events: Events,
    pub twitch_tx: Sender<TwitchAction>,
    pub event_tx: Sender<Event>,

    pub messages: SharedMessages,

    /// Data loaded in from a JSON file.
    pub storage: SharedStorage,

    /// States
    state: State,
    previous_state: Option<State>,

    /// Emote encoding pipeline
    pub emotes: SharedEmotes,
    pub emotes_rx: OSReceiver<(DownloadedEmotes, DownloadedEmotes)>,
    pub decoded_emotes_rx: Option<Receiver<Result<DecodedEmote, String>>>,

    pub running_stream: Option<Child>,
    running_audio: Option<Child>,

    /// Notification and TTS handler
    notification_handler: NotificationHandler,

    /// Latest stream status info (polled periodically)
    stream_info: Option<StreamStatusInfo>,

    /// Multi-channel tab state
    channel_tabs: Vec<String>,
    active_tab: usize,
    toasts: VecDeque<ToastMessage>,
    /// Per-channel saved message history (saved/restored on channel switch)
    message_history: HashMap<String, VecDeque<MessageData>>,
}

macro_rules! shared {
    ($expression:expr) => {
        Rc::new(RefCell::new($expression))
    };
}

impl App {
    fn current_channel(&self) -> String {
        self.channel_tabs
            .get(self.active_tab)
            .cloned()
            .unwrap_or_else(|| self.config.twitch.channel.clone())
    }

    fn refresh_audio_process(&mut self) {
        let Some(process) = self.running_audio.as_mut() else {
            return;
        };

        match process.try_wait() {
            Ok(Some(_)) => self.running_audio = None,
            Ok(None) => {}
            Err(err) => {
                error!("failed checking audio process: {err}");
                self.running_audio = None;
            }
        }
    }

    fn audio_meter(&self) -> String {
        let volume = self.config.frontend.audio_volume.min(100) as usize;
        let filled = volume.div_ceil(10);
        let empty = 10usize.saturating_sub(filled);
        format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
    }

    fn prune_toasts(&mut self) {
        let now = Instant::now();
        self.toasts.retain(|toast| toast.expires_at > now);
    }

    fn push_toast(&mut self, text: impl Into<String>) {
        self.prune_toasts();
        self.toasts.push_back(ToastMessage {
            text: text.into(),
            expires_at: Instant::now() + Duration::from_secs(4),
        });
        while self.toasts.len() > 3 {
            self.toasts.pop_front();
        }
    }

    fn follow_audio_to_current_channel(&mut self) {
        if self.running_audio.is_none() || !self.config.frontend.audio_follow_channel_switch {
            return;
        }

        if let Some(mut process) = self.running_audio.take() {
            _ = process.kill().inspect_err(|err| {
                error!("failed to restart audio process on channel switch: {err}");
            });
        }

        self.toggle_audio();
    }

    pub fn new(
        config: SharedCoreConfig,
        twitch_oauth: TwitchOauth,
        events: Events,
        event_tx: Sender<Event>,
        twitch_tx: Sender<TwitchAction>,
        emotes: Rc<Emotes>,
        decoded_emotes_rx: Option<Receiver<Result<DecodedEmote, String>>>,
    ) -> Self {
        let maximum_messages = config.terminal.maximum_messages;
        let first_state = config.terminal.first_state.clone();
        let initial_channel = config.twitch.channel.clone();

        let storage = shared!(Storage::new(&config));
        let filters = shared!(Filters::new(&config));
        let messages = shared!(VecDeque::with_capacity(maximum_messages));

        let components = Components::builder()
            .config(&config)
            .twitch_oauth(twitch_oauth.clone())
            .event_tx(event_tx.clone())
            .storage(storage.clone())
            .filters(filters)
            .messages(messages.clone())
            .emotes(&emotes)
            .build();

        let emotes_rx = query_emotes(&config, twitch_oauth.clone(), config.twitch.channel.clone());

        let own_login = twitch_oauth
            .login()
            .unwrap_or_else(|| config.twitch.username.clone());
        let notification_handler = NotificationHandler::new(
            config.notifications.clone(),
            config.tts.clone(),
            own_login.clone(),
            twitch_oauth.client(),
            own_login,
        );

        // Spawn background stream info poller (every 60s)
        {
            let channel = config.twitch.channel.clone();
            let poller_client = twitch_oauth.client();
            let poller_tx = event_tx.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    if let Some(ref client) = poller_client {
                        let info = get_stream_info(client, &channel).await;
                        let _ = poller_tx
                            .send(Event::Internal(InternalEvent::StreamInfoUpdate(info)))
                            .await;
                    }
                }
            });
        }

        Self {
            running: true,
            components,
            config,
            twitch_oauth,
            events,
            twitch_tx,
            event_tx,
            messages,
            storage,
            state: first_state,
            previous_state: None,
            emotes,
            emotes_rx,
            decoded_emotes_rx,
            running_stream: None,
            running_audio: None,
            notification_handler,
            stream_info: None,
            channel_tabs: vec![initial_channel],
            active_tab: 0,
            toasts: VecDeque::new(),
            message_history: HashMap::new(),
        }
    }

    fn open_stream(&mut self, channel: &str) {
        self.close_current_stream();
        let view_command = &self.config.frontend.view_command;

        if let Some((command, args)) = view_command.split_first() {
            self.running_stream = Command::new(command.clone())
                .args(args)
                .arg(format!("https://twitch.tv/{channel}"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_or_else(
                    |err| {
                        error!("error spawning view process: {err}");
                        None
                    },
                    Some,
                );
        }
    }

    fn close_current_stream(&mut self) {
        if let Some(process) = self.running_stream.as_mut() {
            _ = process
                .kill()
                .inspect_err(|err| error!("failed to kill view process: {err}"));
        }
        self.running_stream = None;
    }

    fn view_stream_in_terminal(&mut self) {
        self.refresh_audio_process();
        if let Some(mut process) = self.running_audio.take() {
            _ = process.kill().inspect_err(|err| {
                error!("failed to kill audio process before terminal view: {err}");
            });
        }

        let channel = self.current_channel();
        let url = format!("https://twitch.tv/{channel}");

        // Suspend TUI: leave alternate screen and restore terminal
        disable_raw_mode().unwrap_or_default();
        execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture,).unwrap_or_default();

        println!("\n📺 Watching {channel} — press Q to return\n");

        // mpv --vo=tct renders directly into the terminal as true-color blocks
        let mut args = vec![
            "--vo=tct".to_string(),
            "--profile=sw-fast".to_string(),
            format!("--volume={}", self.config.frontend.audio_volume.min(100)),
            "--ytdl-format=best[height<=480]".to_string(),
            "--really-quiet".to_string(),
            "--term-osd-bar".to_string(),
        ];
        apply_mpv_audio_options(
            &mut args,
            &self.config.frontend.audio_output_device,
            &self.config.frontend.audio_client_name,
        );
        args.push(url);

        let mut player = Command::new("mpv");
        apply_audio_client_env(
            &mut player,
            &self.config.frontend.audio_client_name,
            "video",
        );
        let status = player.args(&args).status();

        match status {
            Ok(exit) if exit.success() => {}
            Ok(exit) => {
                eprintln!("⚠ terminal viewer exited: {exit}");
                std::thread::sleep(Duration::from_secs(2));
            }
            Err(e) => {
                eprintln!("⚠ mpv failed: {e}");
                std::thread::sleep(Duration::from_secs(2));
            }
        }

        // Restore TUI
        enable_raw_mode().unwrap_or_default();
        execute!(
            stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBlinking,
        )
        .unwrap_or_default();
    }

    fn toggle_audio(&mut self) {
        self.refresh_audio_process();

        if let Some(mut process) = self.running_audio.take() {
            _ = process
                .kill()
                .inspect_err(|err| error!("failed to kill audio process: {err}"));
            return;
        }
        let channel = self.current_channel();
        let event_tx = self.event_tx.clone();

        let url = format!("https://twitch.tv/{channel}");
        let url_placeholder = concat!("{", "url", "}");

        // Determine which backend to use
        let backend = &self.config.frontend.audio_backend;
        let (cmd, args) = match backend {
            AudioBackend::Mpv => {
                let audio_cmd = self.config.frontend.audio_command.clone();
                if audio_cmd.is_empty() {
                    let _ = event_tx.try_send(
                        DataBuilder::system(
                            "⚠ audio_command is empty — set it in config".to_string(),
                        )
                        .into(),
                    );
                    return;
                }

                // Support {url} placeholder anywhere in args, otherwise append URL at end
                let has_placeholder = audio_cmd.iter().any(|a| a.contains(url_placeholder));
                let args: Vec<String> = if has_placeholder {
                    audio_cmd
                        .iter()
                        .map(|a| a.replace(url_placeholder, &url))
                        .collect()
                } else {
                    let mut a = audio_cmd;
                    a.push(url.clone());
                    a
                };

                let (cmd, mut args) = if args
                    .iter()
                    .any(|arg| arg.starts_with("--volume=") || arg == "--volume")
                {
                    (args[0].clone(), args[1..].to_vec())
                } else {
                    let mut mpv_args = args[1..].to_vec();
                    mpv_args.insert(
                        1,
                        format!("--volume={}", self.config.frontend.audio_volume.min(100)),
                    );
                    (args[0].clone(), mpv_args)
                };

                if cmd == "mpv" || cmd.ends_with("/mpv") {
                    apply_mpv_audio_options(
                        &mut args,
                        &self.config.frontend.audio_output_device,
                        &self.config.frontend.audio_client_name,
                    );
                }

                (cmd, args)
            }
            AudioBackend::Streamlink => {
                // Use streamlink for stream audio with mpv as output
                let volume = self.config.frontend.audio_volume.min(100);
                let mut player_cmd = format!("mpv --no-video --volume={volume}");

                if !self.config.frontend.audio_client_name.is_empty() {
                    let _ = write!(
                        player_cmd,
                        " --audio-client-name={}",
                        shell_escape_arg(&self.config.frontend.audio_client_name)
                    );
                }

                if !self.config.frontend.audio_output_device.is_empty() {
                    let _ = write!(
                        player_cmd,
                        " --audio-device={}",
                        shell_escape_arg(&self.config.frontend.audio_output_device)
                    );
                }

                player_cmd.push_str(" -");
                let cmd = "streamlink".to_string();
                let args = vec![
                    url.clone(),
                    "audio,worst".to_string(), // Try audio-only format first, fallback to worst quality
                    "-o".to_string(),
                    player_cmd, // Pipe to mpv
                ];
                (cmd, args)
            }
        };

        let mut player = Command::new(&cmd);
        apply_audio_client_env(
            &mut player,
            &self.config.frontend.audio_client_name,
            "music",
        );

        match player
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                std::thread::sleep(Duration::from_millis(350));

                match child.try_wait() {
                    Ok(Some(status)) => {
                        let mut details = String::new();
                        if let Some(mut stderr) = child.stderr.take() {
                            let _ = stderr.read_to_string(&mut details);
                        }
                        let details = details
                            .lines()
                            .find(|line| !line.trim().is_empty())
                            .unwrap_or("player exited immediately");
                        let backend_name = match backend {
                            AudioBackend::Mpv => "mpv",
                            AudioBackend::Streamlink => "streamlink",
                        };
                        let _ = event_tx.try_send(
                            DataBuilder::system(format!(
                                "⚠ Audio failed for {channel} ({backend_name}, {status}): {details}"
                            ))
                            .into(),
                        );
                    }
                    Ok(None) => {
                        self.running_audio = Some(child);
                        let backend_name = match backend {
                            AudioBackend::Mpv => "mpv",
                            AudioBackend::Streamlink => "streamlink",
                        };
                        let _ = event_tx.try_send(
                            DataBuilder::system(format!("♪ Audio started ({backend_name}): {url}"))
                                .into(),
                        );
                    }
                    Err(err) => {
                        error!("error checking audio process: {err}");
                        let _ = event_tx.try_send(
                            DataBuilder::system(format!("⚠ Audio failed while starting: {err}"))
                                .into(),
                        );
                    }
                }
            }
            Err(err) => {
                error!("error spawning audio process: {err}");
                let backend_name = match backend {
                    AudioBackend::Mpv => "mpv",
                    AudioBackend::Streamlink => "streamlink",
                };
                let _ = event_tx.try_send(
                    DataBuilder::system(format!(
                        "⚠ Audio failed ({backend_name}, {err}). Is '{cmd}' installed?"
                    ))
                    .into(),
                );
            }
        }
    }

    fn tab_next(&mut self) {
        if self.channel_tabs.len() > 1 {
            self.active_tab = (self.active_tab + 1) % self.channel_tabs.len();
            let channel = self.channel_tabs[self.active_tab].clone();
            let tx = self.twitch_tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(TwitchAction::JoinChannel(channel)).await;
            });
        }
    }

    fn tab_prev(&mut self) {
        if self.channel_tabs.len() > 1 {
            self.active_tab = self
                .active_tab
                .checked_sub(1)
                .unwrap_or(self.channel_tabs.len() - 1);
            let channel = self.channel_tabs[self.active_tab].clone();
            let tx = self.twitch_tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(TwitchAction::JoinChannel(channel)).await;
            });
        }
    }

    fn tab_close(&mut self) {
        if self.channel_tabs.len() <= 1 {
            return;
        }
        self.channel_tabs.remove(self.active_tab);
        if self.active_tab >= self.channel_tabs.len() {
            self.active_tab = self.channel_tabs.len() - 1;
        }
        let channel = self.channel_tabs[self.active_tab].clone();
        let tx = self.twitch_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(TwitchAction::JoinChannel(channel)).await;
        });
    }

    fn cleanup(&mut self) {
        self.close_current_stream();
        if let Some(mut process) = self.running_audio.take() {
            _ = process.kill();
        }
        self.storage.borrow().dump_data();
        self.emotes.unload();
    }

    fn clear_messages(&mut self) {
        self.messages.borrow_mut().clear();

        self.components.chat.scroll_offset.jump_to(0);
    }

    /// Save current channel's messages then load (or clear) messages for `new_channel`.
    fn switch_channel_messages(&mut self, new_channel: &str) {
        let current = self.current_channel();

        // Save the current message list under the current channel name.
        self.message_history
            .insert(current, self.messages.borrow().clone());

        // Restore saved messages for the new channel, or start with an empty list.
        let restored = self
            .message_history
            .get(new_channel)
            .cloned()
            .unwrap_or_default();

        *self.messages.borrow_mut() = restored;
        self.components.chat.scroll_offset.jump_to(0);
    }

    fn purge_user_messages(&self, user_id: &str) {
        let messages = self
            .messages
            .borrow_mut()
            .iter()
            .filter(|&m| m.user_id.clone().is_none_or(|user| user != user_id))
            .cloned()
            .collect::<VecDeque<MessageData>>();

        self.messages.replace(messages);
    }

    fn remove_message_with(&self, message_id: &str) {
        let index = self
            .messages
            .borrow_mut()
            .iter()
            .position(|f| f.message_id.clone().is_some_and(|id| id == message_id));

        if let Some(i) = index {
            self.messages.borrow_mut().remove(i).unwrap();
        }
    }

    fn get_previous_state(&self) -> Option<State> {
        self.previous_state.clone()
    }

    fn set_state(&mut self, other: State) {
        self.previous_state = Some(self.state.clone());
        self.state = other;
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let is_emotes_enabled = self.emotes.enabled;

        while self.running {
            if is_emotes_enabled {
                self.handle_emote_event();
            }

            if let Some(event) = self.events.next().await {
                self.event(&event).await?;
            }

            terminal.draw(|f| self.draw(f, Some(f.area()))).unwrap();
        }

        self.cleanup();

        Ok(())
    }

    fn handle_emote_event(&mut self) {
        // Check if we have received any emotes
        if let Ok((user_emotes, global_emotes)) = self.emotes_rx.try_recv() {
            *self.emotes.user_emotes.borrow_mut() = user_emotes;
            *self.emotes.global_emotes.borrow_mut() = global_emotes;

            for message in &mut *self.messages.borrow_mut() {
                message.reparse_emotes(&self.emotes);
            }
        }

        // Check if we need to load a decoded emote
        if let Some(rx) = &mut self.decoded_emotes_rx {
            if let Ok(r) = rx.try_recv() {
                match r {
                    Ok(d) => {
                        if let Err(e) = d.apply() {
                            warn!("Unable to send command to load emote. {e}");
                        } else if let Err(e) = display_emote(d.id(), 1, d.cols()) {
                            warn!("Unable to send command to display emote. {e}");
                        }
                    }
                    Err(name) => {
                        warn!("Unable to load emote: {name}.");
                        self.emotes.user_emotes.borrow_mut().remove(&name);
                        self.emotes.global_emotes.borrow_mut().remove(&name);
                        self.emotes.info.borrow_mut().remove(&name);
                    }
                }
            }
        }
    }

    fn handle_internal_event(&mut self, internal_event: &InternalEvent) {
        match internal_event {
            InternalEvent::Quit => self.running = false,
            InternalEvent::BackOneLayer => {
                if let Some(previous_state) = self.get_previous_state() {
                    self.set_state(previous_state);
                } else {
                    self.set_state(self.config.terminal.first_state.clone());
                }
            }
            InternalEvent::SwitchState(state) => {
                if self.state == State::Normal {
                    self.clear_messages();
                }

                self.set_state(state.clone());
            }
            InternalEvent::OpenStream(channel) => {
                self.open_stream(channel);
            }
            InternalEvent::SelectEmote(_) => {}
            InternalEvent::CreateClip => {
                let channel = self.config.twitch.channel.clone();
                let client = self.twitch_oauth.client();
                let event_tx = self.event_tx.clone();
                tokio::spawn(async move {
                    let Some(client) = client else {
                        let _ = event_tx
                            .send(
                                DataBuilder::system("⚠ No Twitch client for clip creation".into())
                                    .into(),
                            )
                            .await;
                        return;
                    };
                    match get_channel_id(&client, &channel).await {
                        Ok(broadcaster_id) => match create_clip(&client, &broadcaster_id).await {
                            Ok(clip) => {
                                let url = clip.edit_url.replace("/edit", "");
                                let _ = event_tx
                                    .send(
                                        DataBuilder::system(format!("📎 Clip created: {url}"))
                                            .into(),
                                    )
                                    .await;
                            }
                            Err(e) => {
                                let _ = event_tx
                                    .send(DataBuilder::system(format!("⚠ Clip failed: {e}")).into())
                                    .await;
                            }
                        },
                        Err(e) => {
                            let _ = event_tx
                                .send(
                                    DataBuilder::system(format!("⚠ Could not get channel ID: {e}"))
                                        .into(),
                                )
                                .await;
                        }
                    }
                });
            }
            InternalEvent::StreamInfoUpdate(info) => {
                self.stream_info.clone_from(info);
            }
            InternalEvent::ToggleAudio => {
                self.toggle_audio();
            }
            InternalEvent::ToggleTts => {
                let muted = self.notification_handler.toggle_tts();
                let msg = if muted {
                    "🔇 TTS muted"
                } else {
                    "🔊 TTS enabled"
                };
                let _ = self
                    .event_tx
                    .try_send(DataBuilder::system(msg.to_string()).into());
            }
            InternalEvent::ToggleStreamViewer => {
                self.view_stream_in_terminal();
            }
            InternalEvent::TabNew => {
                // Open channel switcher — user types channel name, then JoinChannel fires
                // We store the new tab when the channel is joined
                let _ = self
                    .event_tx
                    .try_send(Event::Internal(InternalEvent::SwitchState(State::Normal)));
                // Signal chat to open channel_input by sending a fake ChannelSwitcher key
                // The simplest way: just open the channel_switcher via a flag
                // For now, toggle channel_input focus via an event (reuse OpenStream path)
                // We'll wire this up by sending a dedicated message
            }
            InternalEvent::TabNext => {
                self.tab_next();
            }
            InternalEvent::TabPrev => {
                self.tab_prev();
            }
            InternalEvent::TabClose => {
                self.tab_close();
            }
        }
    }

    async fn handle_twitch_action(&mut self, twitch_action: &TwitchAction) -> Result<()> {
        match twitch_action {
            TwitchAction::JoinChannel(channel) => {
                let channel = clean_channel_name(channel);
                self.switch_channel_messages(&channel);
                self.emotes.unload();

                self.twitch_tx
                    .send(TwitchAction::JoinChannel(channel.clone()))
                    .await?;

                if self.config.frontend.autostart_view_command {
                    self.open_stream(&channel);
                }

                // Register channel as a tab if it's new
                if self.channel_tabs.contains(&channel) {
                    self.active_tab = self
                        .channel_tabs
                        .iter()
                        .position(|c| c == &channel)
                        .unwrap_or(0);
                } else {
                    self.channel_tabs.push(channel.clone());
                    self.active_tab = self.channel_tabs.len() - 1;
                }

                self.follow_audio_to_current_channel();

                self.emotes_rx = query_emotes(&self.config, self.twitch_oauth.clone(), channel);
                self.set_state(State::Normal);
            }
            TwitchAction::Message(message) => {
                self.twitch_tx
                    .send(TwitchAction::Message(message.clone()))
                    .await?;
            }
        }

        Ok(())
    }

    fn handle_twitch_notification(&mut self, twitch_notification: &TwitchNotification) {
        match twitch_notification {
            TwitchNotification::Message(m) => {
                if m.system && m.author == "System" {
                    self.push_toast(m.payload.clone());
                    return;
                }

                let message_data = MessageData::from_twitch_message(m.clone(), &self.emotes);
                if !KNOWN_CHATTERS.contains(&message_data.author.as_str())
                    && self.config.twitch.username != message_data.author
                {
                    self.storage
                        .borrow_mut()
                        .add("chatters", message_data.author.clone());
                }

                // Trigger notifications and TTS for non-system messages
                if !m.system {
                    self.notification_handler.play_sound(&m.author, &m.payload);
                    self.notification_handler.speak(&m.author, &m.payload);

                    // Highlight reel: save @mention to log file
                    let own = self
                        .twitch_oauth
                        .login()
                        .unwrap_or_else(|| self.config.twitch.username.clone());
                    if self.config.notifications.highlight_log_enabled
                        && m.payload.to_lowercase().contains(&own.to_lowercase())
                    {
                        self.append_highlight(&m.author, &m.payload);
                    }
                }

                self.messages.borrow_mut().push_front(message_data);

                // If scrolling is enabled, pad for more messages.
                if self.components.chat.scroll_offset.get_offset() > 0 {
                    self.components.chat.scroll_offset.up();
                }
            }
            TwitchNotification::ClearChat(user_id) => {
                if let Some(user) = user_id {
                    self.purge_user_messages(user.as_str());
                } else {
                    self.clear_messages();
                }
            }
            TwitchNotification::DeleteMessage(message_id) => {
                self.remove_message_with(message_id.as_str());
            }
            TwitchNotification::UserJoin(username) => {
                const USER_PLACEHOLDER: &str = "{user}";
                self.notification_handler
                    .play_sound_for_event(EventType::UserJoin, username, "");
                let msg = self
                    .config
                    .notifications
                    .join_message
                    .replace(USER_PLACEHOLDER, username);
                if let TwitchNotification::Message(raw) = DataBuilder::twitch(msg) {
                    self.messages
                        .borrow_mut()
                        .push_front(MessageData::from_twitch_message(raw, &self.emotes));
                }
            }
            TwitchNotification::UserLeave(username) => {
                const USER_PLACEHOLDER: &str = "{user}";
                self.notification_handler
                    .play_sound_for_event(EventType::UserLeave, username, "");
                let msg = self
                    .config
                    .notifications
                    .leave_message
                    .replace(USER_PLACEHOLDER, username);
                if let TwitchNotification::Message(raw) = DataBuilder::twitch(msg) {
                    self.messages
                        .borrow_mut()
                        .push_front(MessageData::from_twitch_message(raw, &self.emotes));
                }
            }
            TwitchNotification::Raid(from_user, viewers) => {
                self.notification_handler
                    .play_sound_for_event(EventType::UserJoin, from_user, "");
                let msg = format!("🚨 RAID! {from_user} is raiding with {viewers} viewer(s)!");
                if let TwitchNotification::Message(raw) = DataBuilder::twitch(msg) {
                    self.messages
                        .borrow_mut()
                        .push_front(MessageData::from_twitch_message(raw, &self.emotes));
                }
            }
        }
    }

    fn append_highlight(&self, author: &str, message: &str) {
        let path = &self.config.notifications.highlight_log_path;
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let channel = &self.config.twitch.channel;
            let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(file, "[{ts}] [{channel}] <{author}> {message}");
        }
    }

    fn draw_channel_tabs(&self, f: &mut Frame, area: Rect) {
        use tui::{text::Line, widgets::Tabs};
        let tab_titles: Vec<Line> = self
            .channel_tabs
            .iter()
            .map(|c| Line::from(c.as_str().to_owned()))
            .collect();
        let tabs = Tabs::new(tab_titles)
            .block(Block::default().style(Style::default().bg(Color::DarkGray)))
            .style(Style::default().fg(Color::White).bg(Color::DarkGray))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .divider("│")
            .select(self.active_tab);
        f.render_widget(tabs, area);
    }

    fn draw_status_bar(&self, f: &mut Frame, area: Rect) {
        let channel = self.current_channel();
        let bar_style = Style::default().bg(Color::DarkGray).fg(Color::White);
        let accent = Style::default()
            .bg(Color::DarkGray)
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let audio_indicator = if self.running_audio.is_some() {
            format!(" ♪{} ", self.audio_meter())
        } else {
            String::new()
        };
        let tts_indicator = if self.notification_handler.is_tts_muted() {
            " 🔇"
        } else {
            ""
        };

        let spans = self.stream_info.as_ref().map_or_else(
            || {
                vec![
                    Span::styled(
                        format!(" 📺 {channel}{audio_indicator}{tts_indicator} "),
                        accent,
                    ),
                    Span::styled("│ ⚫ offline".to_string(), bar_style),
                ]
            },
            |info| {
                let h = info.uptime_secs / 3600;
                let m = (info.uptime_secs % 3600) / 60;
                let uptime = if h > 0 {
                    format!("{h}h{m:02}m")
                } else {
                    format!("{m}m")
                };
                let viewers = info.viewer_count;
                let game = info.game_name.chars().take(20).collect::<String>();
                let title = info.title.chars().take(40).collect::<String>();
                vec![
                    Span::styled(
                        format!(" 📺 {channel}{audio_indicator}{tts_indicator} "),
                        accent,
                    ),
                    Span::styled("│ 🔴 LIVE  ".to_string(), bar_style),
                    Span::styled(format!("👥 {viewers}  "), bar_style),
                    Span::styled(format!("⏱ {uptime}  "), bar_style),
                    Span::styled(format!("🎮 {game}  "), bar_style),
                    Span::styled(title, bar_style),
                ]
            },
        );

        let paragraph = Paragraph::new(Line::from(spans)).block(Block::default().style(bar_style));
        f.render_widget(paragraph, area);
    }

    fn draw_toasts(&mut self, f: &mut Frame) {
        self.prune_toasts();
        if self.toasts.is_empty() {
            return;
        }

        let area = f.area();

        // Max usable content width is 60 columns or 80% of screen, whichever is smaller.
        let max_content_w = ((area.width as usize).saturating_mul(4) / 5).clamp(10, 60);

        // Each toast: word-wrap its text to max_content_w, then height = lines + 2 (borders).
        let toasts: Vec<(Vec<String>, u16)> = self
            .toasts
            .iter()
            .map(|toast| {
                let lines = word_wrap_text(&toast.text, max_content_w);
                let h = (lines.len() as u16 + 2).max(3);
                (lines, h)
            })
            .collect();

        // Derive the render width from the longest actual line across all toasts.
        let max_line_len: usize = toasts
            .iter()
            .flat_map(|(lines, _): &(Vec<String>, u16)| lines.iter())
            .map(|l: &String| l.chars().count())
            .max()
            .unwrap_or(10)
            .min(max_content_w);
        let render_w = (max_line_len as u16 + 4)
            .min(area.width.saturating_sub(2))
            .max(14);

        // Total stack height including 1-line gaps between toasts.
        let gap: u16 = 1;
        let total_h: u16 = toasts
            .iter()
            .map(|(_, h)| h)
            .sum::<u16>()
            .saturating_add(gap * toasts.len().saturating_sub(1) as u16);

        // Center the whole stack on screen.
        let start_x = area.x + area.width.saturating_sub(render_w) / 2;
        let start_y = area.y + area.height.saturating_sub(total_h) / 2;

        let border_type = self.config.frontend.border_type.clone().into();
        let mut cursor_y = start_y;

        for (toast_lines, toast_h) in &toasts {
            if cursor_y + toast_h > area.y + area.height {
                break;
            }
            let rect = Rect::new(start_x, cursor_y, render_w, *toast_h);
            let text: String = toast_lines.join("\n");
            let paragraph = Paragraph::new(text)
                .block(
                    Block::default()
                        .borders(tui::widgets::Borders::ALL)
                        .border_type(border_type)
                        .style(Style::default().bg(Color::Rgb(30, 30, 30)).fg(Color::White)),
                )
                .alignment(tui::layout::Alignment::Center)
                .wrap(Wrap { trim: false });
            f.render_widget(Clear, rect);
            f.render_widget(paragraph, rect);
            cursor_y += toast_h + gap;
        }
    }
}

/// Word-wrap `text` to `max_width` columns, returning a Vec of lines.
fn word_wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            let cur_len = current.chars().count();
            if current.is_empty() {
                if word_len > max_width {
                    // Force-break a single word that's too long
                    let chars = word.chars();
                    let mut chunk = String::new();
                    for c in chars {
                        chunk.push(c);
                        if chunk.chars().count() == max_width {
                            lines.push(chunk.clone());
                            chunk.clear();
                        }
                    }
                    current = chunk;
                } else {
                    current = word.to_string();
                }
            } else if cur_len + 1 + word_len <= max_width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current.clone());
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn shell_escape_arg(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

impl Component for App {
    fn draw(&mut self, f: &mut Frame, _area: Option<Rect>) {
        let full_area = f.area();
        // Reserve 1 line at the bottom for the stream status bar
        let [main_area, status_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .areas(full_area);

        // Reserve 1 line at the top for channel tabs (only when multiple tabs open)
        let (tabs_area, content_area) = if self.channel_tabs.len() > 1 {
            let [tabs, content] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1)])
                .areas(main_area);
            (Some(tabs), content)
        } else {
            (None, main_area)
        };

        let mut size = content_area;

        if self.config.frontend.state_tabs {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(size.height - 1), Constraint::Length(1)])
                .split(content_area);

            size = layout[0];
            self.components.tabs.draw(f, Some(layout[1]), &self.state);
        }

        if (size.height < 10 || size.width < 60)
            && self.config.frontend.show_unsupported_screen_size
        {
            self.components.window_size_error.draw(f, Some(f.area()));
        } else {
            match self.state {
                State::Dashboard => self.components.dashboard.draw(f, Some(size)),
                State::Normal => self.components.chat.draw(f, Some(size)),
                State::Help => self.components.help.draw(f, Some(size)),
            }
        }

        if self.components.debug.is_focused() {
            let new_rect = Rect::new(size.x, size.y + 1, size.width - 1, size.height - 2);

            let rect = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(new_rect)[1];

            self.components.debug.draw(f, Some(rect));
        }

        // Channel tabs bar
        if let Some(tabs_rect) = tabs_area {
            self.draw_channel_tabs(f, tabs_rect);
        }

        // Stream status bar
        self.draw_status_bar(f, status_area);
        self.draw_toasts(f);
    }

    async fn event(&mut self, event: &Event) -> Result<()> {
        self.refresh_audio_process();

        match event {
            Event::Internal(internal_event) => {
                self.handle_internal_event(internal_event);
            }
            Event::Twitch(twitch_event) => match twitch_event {
                TwitchEvent::Action(twitch_action) => {
                    self.handle_twitch_action(twitch_action).await?;
                }
                TwitchEvent::Notification(twitch_notification) => {
                    self.handle_twitch_notification(twitch_notification);
                }
            },
            Event::Input(key) => {
                if self.components.debug.is_focused() {
                    return self.components.debug.event(event).await;
                }

                if self.config.keybinds.toggle_debug_focus.contains(key) {
                    self.components.debug.toggle_focus();
                }
            }
            Event::Tick => {}
        }

        match self.state {
            State::Dashboard => self.components.dashboard.event(event).await,
            State::Normal => self.components.chat.event(event).await,
            State::Help => self.components.help.event(event).await,
        }
    }
}
