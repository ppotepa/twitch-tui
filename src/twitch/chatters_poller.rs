use std::{collections::HashSet, sync::Arc, time::Duration};

use reqwest::Client;
use tokio::sync::{mpsc::Sender, watch};
use tracing::{info, warn};

use crate::{
    events::{Event, TwitchEvent, TwitchNotification},
    handlers::data::DataBuilder,
    twitch::api::chatters::get_chatters,
};

pub struct ChattersPoller {
    client: Client,
    channel_rx: watch::Receiver<Option<(String, String)>>,
    event_tx: Sender<Event>,
    poll_interval: Duration,
}

impl ChattersPoller {
    pub fn new(
        client: Client,
        channel_rx: watch::Receiver<Option<(String, String)>>,
        event_tx: Sender<Event>,
        poll_interval_secs: u64,
    ) -> Self {
        Self {
            client,
            channel_rx,
            event_tx,
            poll_interval: Duration::from_secs(poll_interval_secs),
        }
    }

    pub fn spawn(self) {
        tokio::task::spawn(async move { self.run().await });
    }

    async fn send_system(&self, msg: impl Into<String>) {
        let _ = self
            .event_tx
            .send(DataBuilder::system(msg.into()).into())
            .await;
    }

    async fn run(mut self) {
        let mut known: HashSet<String> = HashSet::new();
        let mut current_channel: Option<(String, String)> = None;
        let mut first_poll = true;

        loop {
            // borrow_and_update marks as seen so channel_rx.changed() fires on next update
            let new_channel = self.channel_rx.borrow_and_update().clone();

            if new_channel != current_channel {
                if new_channel.is_some() {
                    info!("Chatters poller: channel changed, resetting viewer list");
                }
                current_channel = new_channel;
                known.clear();
                first_poll = true;
            }

            if let Some((ref broadcaster_id, ref user_id)) = current_channel {
                match get_chatters(&self.client, broadcaster_id, user_id).await {
                    Ok(chatters) => {
                        let new_set: HashSet<String> =
                            chatters.into_iter().map(|c| c.user_login).collect();

                        if first_poll {
                            // Seed silently, just confirm it's running
                            self.send_system(format!(
                                "👥 Chatters tracking active — {} viewer(s) in channel",
                                new_set.len()
                            ))
                            .await;
                        } else {
                            // Viewers who appeared since last poll
                            for user in new_set.difference(&known) {
                                let _ = self
                                    .event_tx
                                    .send(Event::Twitch(TwitchEvent::Notification(
                                        TwitchNotification::UserJoin(user.clone()),
                                    )))
                                    .await;
                            }
                            // Viewers who left since last poll
                            for user in known.difference(&new_set) {
                                let _ = self
                                    .event_tx
                                    .send(Event::Twitch(TwitchEvent::Notification(
                                        TwitchNotification::UserLeave(user.clone()),
                                    )))
                                    .await;
                            }
                        }

                        known = new_set;
                        first_poll = false;
                    }
                    Err(err) => {
                        warn!("Chatters poller: failed to fetch chatters: {err}");
                        // Surface the error so the user can see it in the chat view
                        self.send_system(format!(
                            "⚠ Chatters tracking error: {err} (requires moderator:read:chatters scope)"
                        ))
                        .await;
                        // Back off before retrying to avoid spamming on repeated failures
                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                }
            }

            // Wait for poll interval OR an immediate channel change — whichever comes first
            tokio::select! {
                _ = tokio::time::sleep(self.poll_interval) => {}
                result = self.channel_rx.changed() => {
                    if result.is_err() {
                        // Sender dropped — app is shutting down
                        return;
                    }
                }
            }
        }
    }
}

pub type ChannelWatchTx = Arc<watch::Sender<Option<(String, String)>>>;
