#![forbid(unsafe_code)]
#![warn(clippy::nursery, clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::module_name_repetitions,
    clippy::struct_excessive_bools,
    clippy::future_not_send,
    clippy::suboptimal_flops,
    clippy::too_many_lines,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::struct_field_names,
    clippy::too_many_arguments
)]

use std::{rc::Rc, sync::Arc};

use clap::Parser;
use color_eyre::eyre::{Result, WrapErr};
use logging::initialize_logging;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::{
    app::App,
    boot::run_boot_screen,
    cli::args::Cli,
    commands::{init_terminal, reset_terminal},
    config::CoreConfig,
    emotes::{Emotes, initialize_emote_decoder},
    events::{Event, Events, TwitchAction},
    twitch::{chatters_poller::ChattersPoller, oauth::TwitchOauth, websocket::TwitchWebsocket},
};

mod app;
mod audio;
mod boot;
mod cli;
mod commands;
mod config;
mod emotes;
mod events;
mod handlers;
mod logging;
mod notifications;
mod tts;
mod twitch;
mod ui;
mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let mut config = CoreConfig::new(Cli::parse()).wrap_err("Configuration error.")?;

    initialize_logging(&config).wrap_err("Failed to initialize logger")?;

    info!("Logging system initialised");

    let (event_tx, event_rx) = mpsc::channel::<Event>(100);
    let (twitch_tx, twitch_rx) = mpsc::channel::<TwitchAction>(100);

    let emotes = initialize_emote_decoder(&mut config);

    let config = Arc::new(config);

    let twitch_oauth = TwitchOauth::default().init(config.clone()).await?;
    let emotes_enabled = config.frontend.is_emotes_enabled();
    let context_emotes = Rc::new(Emotes::new(emotes_enabled));

    let events = Events::new(config.terminal.delay, event_tx.clone(), event_rx);

    let decoded_emotes_rx = if let Some((rx, cell_size)) = emotes {
        context_emotes.cell_size.get_or_init(|| cell_size);
        Some(rx)
    } else {
        None
    };

    let app = App::new(
        config.clone(),
        twitch_oauth.clone(),
        events,
        event_tx.clone(),
        twitch_tx,
        context_emotes,
        decoded_emotes_rx,
    );

    // Shared channel for notifying the chatters poller when the active channel changes
    let (channel_watch_tx, channel_watch_rx) = watch::channel(None::<(String, String)>);

    TwitchWebsocket::new(
        config.clone(),
        twitch_oauth.clone(),
        event_tx.clone(),
        twitch_rx,
        Arc::new(channel_watch_tx),
    );

    // Spawn the chatters poller if the client is available
    if let Some(client) = twitch_oauth.client() {
        let poll_interval = config.notifications.viewer_poll_interval_secs;
        ChattersPoller::new(client, channel_watch_rx, event_tx.clone(), poll_interval).spawn();
    } else {
        warn!("Twitch client not available, chatters poller will not run");
    }

    let mut terminal = init_terminal(&config.frontend);
    run_boot_screen(&mut terminal, &config, &twitch_oauth).await;
    app.run(terminal).await?;

    reset_terminal();

    std::process::exit(0)
}
