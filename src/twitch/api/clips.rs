use color_eyre::{Result, eyre::ContextCompat};
use reqwest::Client;
use serde::Deserialize;

use super::TWITCH_API_BASE_URL;

#[derive(Deserialize, Debug, Clone)]
pub struct Clip {
    #[allow(dead_code)]
    pub id: String,
    pub edit_url: String,
}

#[derive(Deserialize)]
struct ClipResponse {
    data: Vec<Clip>,
}

/// Create a clip of the broadcaster's current stream.
///
/// Requires `clips:edit` scope.
/// <https://dev.twitch.tv/docs/api/reference/#create-clip>
pub async fn create_clip(client: &Client, broadcaster_id: &str) -> Result<Clip> {
    let url = format!("{TWITCH_API_BASE_URL}/clips?broadcaster_id={broadcaster_id}");

    let clip = client
        .post(&url)
        .send()
        .await?
        .error_for_status()?
        .json::<ClipResponse>()
        .await?
        .data
        .into_iter()
        .next()
        .context("No clip returned from API")?;

    Ok(clip)
}
