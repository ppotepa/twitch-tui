use color_eyre::{Result, eyre::ContextCompat};
use reqwest::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use super::TWITCH_API_BASE_URL;

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Channel {
    id: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct ChannelList {
    data: Vec<Channel>,
}

/// Gets the channel ID of the specified channel name
///
/// <https://dev.twitch.tv/docs/api/reference/#get-users>
pub async fn get_channel_id(client: &Client, channel: &str) -> Result<String> {
    let response = client
        .get(format!("{TWITCH_API_BASE_URL}/users?login={channel}"))
        .send()
        .await?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(std::io::Error::other(format!("Channel '{channel}' was not found")).into());
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(std::io::Error::other(format!(
            "Failed to resolve channel '{channel}' via /users lookup: {status} {body}"
        ))
        .into());
    }

    let response_channel_id = response
        .json::<ChannelList>()
        .await?
        .data
        .first()
        .context("Could not get channel id.")?
        .id
        .clone();

    Ok(response_channel_id)
}
