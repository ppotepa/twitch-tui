use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

use super::TWITCH_API_BASE_URL;
use crate::events::StreamStatusInfo;

#[derive(Deserialize)]
struct StreamsResponse {
    data: Vec<serde_json::Value>,
}

#[derive(Deserialize, Debug, Clone)]
struct StreamDetail {
    viewer_count: u64,
    title: String,
    game_name: String,
    started_at: String,
}

#[derive(Deserialize)]
struct StreamsDetailResponse {
    data: Vec<StreamDetail>,
}

/// Returns true if the given channel currently has a live stream.
pub async fn is_stream_live(client: &Client, user_login: &str) -> bool {
    let url = format!("{TWITCH_API_BASE_URL}/streams?user_login={user_login}&first=1");
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => resp
            .json::<StreamsResponse>()
            .await
            .map(|r| !r.data.is_empty())
            .unwrap_or(false),
        _ => false,
    }
}

/// Returns stream info for the given channel, or None if the channel is offline.
pub async fn get_stream_info(client: &Client, user_login: &str) -> Option<StreamStatusInfo> {
    let url = format!("{TWITCH_API_BASE_URL}/streams?user_login={user_login}&first=1");
    let detail = client
        .get(&url)
        .send()
        .await
        .ok()?
        .json::<StreamsDetailResponse>()
        .await
        .ok()?
        .data
        .into_iter()
        .next()?;

    let uptime_secs = DateTime::parse_from_rfc3339(&detail.started_at)
        .ok()
        .map(|start| {
            Utc::now()
                .signed_duration_since(start.with_timezone(&Utc))
                .num_seconds()
                .max(0) as u64
        })
        .unwrap_or(0);

    Some(StreamStatusInfo {
        viewer_count: detail.viewer_count,
        title: detail.title,
        game_name: detail.game_name,
        uptime_secs,
    })
}
