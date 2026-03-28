use std::time::Duration;

use rodio::OutputStream;
use tui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyEventKind},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{config::CoreConfig, tts::TtsProvider, twitch::oauth::TwitchOauth};

#[derive(Debug, Clone)]
enum Status {
    Pending,
    Running,
    Ok(String),
    Warn(String),
    Fail(String),
}

#[derive(Debug, Clone)]
struct Check {
    label: String,
    status: Status,
}

impl Check {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: Status::Pending,
        }
    }
}

fn render_frame(f: &mut Frame, checks: &[Check], done: bool, version: &str) {
    let area = f.area();

    let content_height = (checks.len() as u16) + 8;
    let content_width = 66u16.min(area.width.saturating_sub(4));

    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(content_height),
            Constraint::Fill(1),
        ])
        .split(area);

    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(content_width),
            Constraint::Fill(1),
        ])
        .split(vert[1]);

    let panel = horiz[1];

    let mut lines: Vec<Line> = vec![Line::raw("")];

    for check in checks {
        let line = match &check.status {
            Status::Pending => Line::from(vec![
                Span::styled("  [    ] ", Style::default().fg(Color::DarkGray)),
                Span::styled(check.label.clone(), Style::default().fg(Color::DarkGray)),
            ]),
            Status::Running => Line::from(vec![
                Span::styled("  [", Style::default().fg(Color::Yellow)),
                Span::styled(
                    "....",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("] ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    check.label.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Status::Ok(detail) => Line::from(vec![
                Span::styled(
                    "  [ ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "OK",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " ] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(check.label.clone(), Style::default().fg(Color::White)),
                if detail.is_empty() {
                    Span::raw("")
                } else {
                    Span::styled(
                        format!("  {detail}"),
                        Style::default().fg(Color::DarkGray),
                    )
                },
            ]),
            Status::Warn(detail) => Line::from(vec![
                Span::styled("  [", Style::default().fg(Color::Yellow)),
                Span::styled(
                    "WARN",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("] ", Style::default().fg(Color::Yellow)),
                Span::styled(check.label.clone(), Style::default().fg(Color::White)),
                Span::styled(
                    format!("  {detail}"),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Status::Fail(detail) => Line::from(vec![
                Span::styled("  [", Style::default().fg(Color::Red)),
                Span::styled(
                    "FAIL",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled("] ", Style::default().fg(Color::Red)),
                Span::styled(check.label.clone(), Style::default().fg(Color::White)),
                Span::styled(
                    format!("  {detail}"),
                    Style::default().fg(Color::Red),
                ),
            ]),
        };
        lines.push(line);
    }

    lines.push(Line::raw(""));

    if done {
        let fails = checks
            .iter()
            .filter(|c| matches!(c.status, Status::Fail(_)))
            .count();
        if fails > 0 {
            lines.push(Line::from(Span::styled(
                format!("  ⚠  {fails} check(s) failed — some features may not work"),
                Style::default().fg(Color::Yellow),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "  ✓  All checks passed",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        lines.push(Line::from(Span::styled(
            "  Press any key to continue...",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::raw(""));

    let title = format!(" twitch-tui {version} — Diagnostics ");

    let para = Paragraph::new(lines).block(
        Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(para, panel);
}

async fn cmd_exists(name: &str) -> bool {
    tokio::process::Command::new("which")
        .arg(name)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build the full ordered list of checks based on config.
fn build_checks(config: &CoreConfig) -> Vec<Check> {
    let mut checks = vec![
        Check::new("Configuration"),
        Check::new("Twitch token"),
        Check::new("Twitch authentication"),
        Check::new("Scope: moderator:read:chatters"),
        Check::new("Audio system"),
        Check::new("Audio routing"),
    ];

    if config.tts.enabled {
        for provider in config.tts.ordered_providers() {
            checks.push(Check::new(match provider {
                TtsProvider::Festival => "TTS: festival",
                TtsProvider::EspeakNg => "TTS: espeak-ng",
                TtsProvider::GoogleCloud => "TTS: google-cloud",
                TtsProvider::EdgeTts => "TTS: edge-tts",
            }));
        }
    }

    checks
}

pub async fn run_boot_screen(
    terminal: &mut DefaultTerminal,
    config: &CoreConfig,
    twitch_oauth: &TwitchOauth,
) {
    let version = env!("CARGO_PKG_VERSION");
    let mut checks = build_checks(config);

    // Show all checks as pending initially
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));

    // ── Check 0: Configuration ──────────────────────────────────────────────
    checks[0].status = Status::Running;
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));
    tokio::time::sleep(Duration::from_millis(60)).await;
    checks[0].status = Status::Ok("loaded".into());
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));

    // ── Check 1: Twitch token ───────────────────────────────────────────────
    checks[1].status = Status::Running;
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));
    tokio::time::sleep(Duration::from_millis(40)).await;
    checks[1].status = if config.twitch.token.is_some() {
        Status::Ok("present".into())
    } else {
        Status::Fail("no token in config".into())
    };
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));

    // ── Check 2: OAuth / login ──────────────────────────────────────────────
    checks[2].status = Status::Running;
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));
    tokio::time::sleep(Duration::from_millis(40)).await;
    checks[2].status = if let Some(login) = twitch_oauth.login() {
        Status::Ok(login)
    } else {
        Status::Fail("OAuth not validated".into())
    };
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));

    // ── Check 3: moderator:read:chatters scope ──────────────────────────────
    checks[3].status = Status::Running;
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));
    tokio::time::sleep(Duration::from_millis(40)).await;
    checks[3].status = if twitch_oauth.has_scope("moderator:read:chatters") {
        Status::Ok(String::new())
    } else {
        Status::Warn("missing — viewer join/leave tracking disabled".into())
    };
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));

    // ── Check 4: Audio system ───────────────────────────────────────────────
    checks[4].status = Status::Running;
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));
    let audio_ok = tokio::task::spawn_blocking(|| OutputStream::try_default().is_ok())
        .await
        .unwrap_or(false);
    checks[4].status = if audio_ok {
        Status::Ok("rodio ready".into())
    } else {
        Status::Warn("no audio device — sounds disabled".into())
    };
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));

    // ── Check 5: Audio routing ──────────────────────────────────────────────
    checks[5].status = Status::Running;
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));
    tokio::time::sleep(Duration::from_millis(40)).await;
    
    let stream_backend = &config.frontend.audio_backend;
    let backend_name = match stream_backend {
        crate::config::AudioBackend::Mpv => "mpv",
        crate::config::AudioBackend::Streamlink => "streamlink",
    };
    
    let routing_info = if config.frontend.audio_obs_mode {
        format!("{} (OBS mode: unified)", backend_name)
    } else {
        format!("{} (normal mode)", backend_name)
    };
    
    let backend_ok = if backend_name == "streamlink" {
        cmd_exists("streamlink").await
    } else {
        cmd_exists("mpv").await
    };
    
    checks[5].status = if backend_ok {
        Status::Ok(routing_info)
    } else {
        Status::Warn(format!("{}: not found", backend_name))
    };
    let _ = terminal.draw(|f| render_frame(f, &checks, false, version));

    // ── TTS checks (dynamic, if tts.enabled) ───────────────────────────────
    if config.tts.enabled {
        let providers = config.tts.ordered_providers();
        let tts_start = 6usize;

        for (i, provider) in providers.iter().enumerate() {
            let idx = tts_start + i;
            checks[idx].status = Status::Running;
            let _ = terminal.draw(|f| render_frame(f, &checks, false, version));

            checks[idx].status = match provider {
                TtsProvider::Festival => {
                    if cmd_exists("festival").await {
                        Status::Ok("found".into())
                    } else {
                        Status::Fail("not found — install festival".into())
                    }
                }
                TtsProvider::EspeakNg => {
                    if cmd_exists("espeak-ng").await {
                        Status::Ok("found".into())
                    } else {
                        Status::Fail("not found — install espeak-ng".into())
                    }
                }
                TtsProvider::GoogleCloud => {
                    if std::env::var("GOOGLE_API_KEY").is_ok() {
                        Status::Ok("GOOGLE_API_KEY set".into())
                    } else {
                        Status::Warn("GOOGLE_API_KEY not set".into())
                    }
                }
                TtsProvider::EdgeTts => {
                    if cmd_exists("edge-tts").await {
                        Status::Ok("found".into())
                    } else {
                        Status::Fail("not found — pip install edge-tts".into())
                    }
                }
            };
            let _ = terminal.draw(|f| render_frame(f, &checks, false, version));
        }
    }

    // ── Done ────────────────────────────────────────────────────────────────
    let _ = terminal.draw(|f| render_frame(f, &checks, true, version));

    // Wait up to 3s or keypress to continue
    let _ = tokio::task::spawn_blocking(|| {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            if event::poll(remaining.min(Duration::from_millis(100))).unwrap_or(false) {
                if let Ok(Event::Key(k)) = event::read() {
                    if k.kind == KeyEventKind::Press {
                        break;
                    }
                }
            }
        }
    })
    .await;
}
