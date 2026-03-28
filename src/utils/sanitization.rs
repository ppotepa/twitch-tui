/// Extract just the channel login from a raw or formatted string.
///
/// Handles two formats:
/// - Legacy colon-separated: `"xithrius    : stream name"` → `"xithrius"`
/// - Live-channel display:   `"bulmyeolja       102👥  [game]  title"` → `"bulmyeolja"`
///
/// Always lowercases and trims the result.
pub fn clean_channel_name(channel: &str) -> String {
    // Strip legacy "login : title" format first, then take the first whitespace token.
    let base = channel.split_once(':').map_or(channel, |(a, _)| a).trim();

    base.split_whitespace()
        .next()
        .unwrap_or(base)
        .to_lowercase()
}

#[test]
fn test_clean_channel_already_clean() {
    let channel = "xithrius";
    let cleaned_channel = clean_channel_name(channel);
    assert_eq!(cleaned_channel, channel);
}

#[test]
fn test_clean_channel_non_clean_channel() {
    let channel = "xithrius    : stream name";
    let cleaned_channel = clean_channel_name(channel);
    assert_eq!(cleaned_channel, "xithrius");
}

#[test]
fn test_clean_channel_live_display_format() {
    let channel =
        "bulmyeolja               102\u{1f465}  [starcraft ii]         shin vs cure - sel #21";
    let cleaned_channel = clean_channel_name(channel);
    assert_eq!(cleaned_channel, "bulmyeolja");
}

#[test]
fn test_clean_channel_uppercase() {
    let channel = "Xithrius";
    let cleaned_channel = clean_channel_name(channel);
    assert_eq!(cleaned_channel, "xithrius");
}
