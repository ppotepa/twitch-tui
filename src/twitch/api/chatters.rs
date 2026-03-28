use color_eyre::Result;
use reqwest::Client;
use serde::Deserialize;

use super::TWITCH_API_BASE_URL;

#[derive(Deserialize, Debug, Clone)]
pub struct Chatter {
    pub user_login: String,
    #[allow(dead_code)]
    pub user_name: String,
}

#[derive(Deserialize)]
struct ChattersResponse {
    data: Vec<Chatter>,
}

/// Fetch current chatters in a channel.
///
/// Requires the `moderator:read:chatters` OAuth scope.
/// <https://dev.twitch.tv/docs/api/reference/#get-chatters>
pub async fn get_chatters(
    client: &Client,
    broadcaster_id: &str,
    moderator_id: &str,
) -> Result<Vec<Chatter>> {
    let url = format!(
        "{TWITCH_API_BASE_URL}/chat/chatters?broadcaster_id={broadcaster_id}&moderator_id={moderator_id}&first=1000"
    );

    let chatters = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json::<ChattersResponse>()
        .await?
        .data;

    Ok(chatters)
}
