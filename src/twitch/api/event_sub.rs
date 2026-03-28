use std::collections::HashMap;

use ::std::hash::BuildHasher;
use color_eyre::{
    Result,
    eyre::{Context, ContextCompat},
};
use reqwest::{Client, StatusCode};
use tracing::{debug, error, warn};

use super::TWITCH_API_BASE_URL;
use crate::twitch::{
    api::subscriptions::Subscription,
    models::{ReceivedTwitchSubscription, TwitchSubscriptionResponse},
    oauth::TwitchOauth,
};

/// Events that should be subscribed to when the first chat room is entered.
/// Channel chat messages are excluded since it's subscribed to on channel join.
pub static INITIAL_EVENT_SUBSCRIPTIONS: &[Subscription] = &[
    Subscription::Message,
    Subscription::Notification,
    Subscription::Clear,
    Subscription::ClearUserMessages,
    Subscription::MessageDelete,
];

/// Subscribe to a set of events, returning a hashmap of subscription types corresponding to their ID
///
/// <https://dev.twitch.tv/docs/api/reference/#create-eventsub-subscription>
///
/// Different subscription types
///
/// <https://dev.twitch.tv/docs/eventsub/eventsub-subscription-types/#subscription-types>
///
/// No need to delete a subscription if/when session ends, since they're disabled automatically
///
/// <https://dev.twitch.tv/docs/eventsub/handling-websocket-events/#which-events-is-my-websocket-subscribed-to>
pub async fn subscribe_to_events(
    client: &Client,
    oauth: &TwitchOauth,
    session_id: Option<String>,
    channel_id: String,
    subscription_types: Vec<Subscription>,
) -> Result<HashMap<Subscription, String>> {
    let session_id = session_id.context("Session ID is empty")?;

    let url = format!("{TWITCH_API_BASE_URL}/eventsub/subscriptions");

    let user_id = oauth
        .user_id()
        .context("Faield to get user ID from twitch OAuth context")?;

    let mut subscription =
        ReceivedTwitchSubscription::new(channel_id.clone(), user_id.clone(), session_id.clone());

    let mut subscription_map = HashMap::new();

    for subscription_type in subscription_types {
        subscription.set_subscription_type(subscription_type.clone());

        let response = client.post(&url).json(&subscription).send().await?;

        if response.status() == StatusCode::CONFLICT {
            error!("Conflict on event subscription: already subscribed to {subscription_type}");
            match find_existing_subscription_id(
                client,
                &url,
                &subscription_type,
                &channel_id,
                &user_id,
                &session_id,
            )
            .await
            {
                Ok(existing_id) => {
                    debug!("Reusing existing event subscription {subscription_type}");
                    subscription_map.insert(subscription_type, existing_id);
                }
                Err(err) if is_required_subscription(&subscription_type) => return Err(err),
                Err(err) => warn!(
                    "Skipping optional event subscription {subscription_type} after conflict: {err}"
                ),
            }
            continue;
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let mut message = format!(
                "Failed to subscribe to {subscription_type} for channel {channel_id}: {status} {body}"
            );

            if status == StatusCode::FORBIDDEN && matches!(subscription_type, Subscription::Message)
            {
                message.push_str(
                    " — Twitch usually returns this when the account is banned/timed out in that chat, lacks proper authorization, or has hit a join limit.",
                );
            }

            let err = std::io::Error::other(message);

            if is_required_subscription(&subscription_type) {
                return Err(err.into());
            }

            warn!("{err}");
            continue;
        }

        let response_data = response
            .json::<TwitchSubscriptionResponse>()
            .await
            .context(format!(
                "Could not deserialize {subscription_type} event subscription response"
            ))?
            .data();
        let subscription_id = response_data
            .first()
            .context("Could not get channel subscription data")?
            .id()
            .context("Could not get ID from Twitch subscription data")?;

        debug!("Subscribed to event {subscription_type}");

        subscription_map.insert(subscription_type, subscription_id.clone());
    }

    if !subscription_map.contains_key(&Subscription::Message) {
        return Err(std::io::Error::other(format!(
            "Failed to subscribe to required channel.chat.message stream for channel {channel_id}"
        ))
        .into());
    }

    Ok(subscription_map)
}

const fn is_required_subscription(subscription_type: &Subscription) -> bool {
    matches!(subscription_type, Subscription::Message)
}

async fn find_existing_subscription_id(
    client: &Client,
    url: &str,
    subscription_type: &Subscription,
    channel_id: &str,
    user_id: &str,
    session_id: &str,
) -> Result<String> {
    let response_data = client
        .get(url)
        .query(&[("type", subscription_type.to_string())])
        .send()
        .await?
        .error_for_status()?
        .json::<TwitchSubscriptionResponse>()
        .await?
        .data();

    response_data
        .into_iter()
        .find(|subscription| {
            subscription.subscription_type() == Some(subscription_type)
                && subscription.condition().broadcaster_user_id() == channel_id
                && subscription.condition().user_id() == user_id
                && subscription.transport().session_id() == session_id
                && subscription.status().is_some_and(|status| status == "enabled")
        })
        .and_then(|subscription| subscription.id().cloned())
        .context(format!(
            "Could not find existing {subscription_type} subscription for channel {channel_id} on current session"
        ))
}

/// Removes a subscription from the current session
///
/// <https://dev.twitch.tv/docs/api/reference/#delete-eventsub-subscription>
pub async fn unsubscribe_from_events<S: BuildHasher>(
    client: &Client,
    subscriptions: &HashMap<Subscription, String, S>,
    remove_subscription_types: Vec<Subscription>,
) -> Result<()> {
    let url = format!("{TWITCH_API_BASE_URL}/eventsub/subscriptions");

    for subscription_type in remove_subscription_types {
        let Some(subscription_id) = subscriptions.get(&subscription_type) else {
            continue;
        };

        let response = client
            .delete(&url)
            .query(&[("id", subscription_id)])
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            warn!("Event subscription {subscription_type} was already gone");
            continue;
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("Failed to unsubscribe from {subscription_type}: {status} {body}");
            continue;
        }

        debug!("Unsubscribed from event {subscription_type}");
    }

    Ok(())
}
