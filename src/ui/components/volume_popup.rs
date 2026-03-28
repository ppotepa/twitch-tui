use tui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{events::Key, ui::components::utils::popup_area};

const BAR_WIDTH: usize = 20;

/// Result returned from `VolumePopup::handle_key`.
pub enum PopupResult {
    /// The popup remains open and no external action is needed.
    Continue,
    /// The user confirmed — caller should save & close the popup.
    Save,
    /// The user cancelled — caller should discard & close the popup.
    Cancel,
}

#[derive(Clone, PartialEq, Eq)]
enum ItemKind {
    Volume,
    Toggle,
}

struct Item {
    label: &'static str,
    kind: ItemKind,
    value: u8,   // 0-100 for Volume, 0/1 for Toggle
    hint: &'static str,
}

impl Item {
    fn volume(label: &'static str, value: u8, hint: &'static str) -> Self {
        Self { label, kind: ItemKind::Volume, value, hint }
    }
    fn toggle(label: &'static str, enabled: bool, hint: &'static str) -> Self {
        Self { label, kind: ItemKind::Toggle, value: u8::from(enabled), hint }
    }
    fn is_on(&self) -> bool { self.value != 0 }
}

pub struct VolumePopup {
    items: Vec<Item>,
    selected: usize,
}

impl VolumePopup {
    pub fn new(
        stream_audio: u8,
        tts_volume: u8,
        notif_volume: u8,
        spatial: bool,
    ) -> Self {
        Self {
            items: vec![
                Item::volume("Stream Audio",  stream_audio, "Stream / mpv playback level"),
                Item::volume("TTS Volume",    tts_volume,   "Text-to-speech output level"),
                Item::volume("Notif Volume",  notif_volume, "Event sounds (join/leave/raid)"),
                Item::toggle("Spatial TTS",   spatial,      "3-D positional audio mode"),
            ],
            selected: 0,
        }
    }

    // ── accessors ──────────────────────────────────────────────────────────────

    pub fn stream_audio(&self)  -> u8   { self.items[0].value }
    pub fn tts_volume(&self)    -> u8   { self.items[1].value }
    pub fn notif_volume(&self)  -> u8   { self.items[2].value }
    pub fn spatial_enabled(&self) -> bool { self.items[3].is_on() }

    // ── input ──────────────────────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: &Key) -> PopupResult {
        match key {
            Key::Up => {
                if self.selected > 0 { self.selected -= 1; }
            }
            Key::Down => {
                if self.selected + 1 < self.items.len() { self.selected += 1; }
            }
            Key::Left => self.adjust(-5),
            Key::Right => self.adjust(5),
            Key::Char(' ') | Key::Char('\n') => {
                let item = &mut self.items[self.selected];
                if item.kind == ItemKind::Toggle {
                    item.value = 1 - item.value;
                }
            }
            Key::Enter => return PopupResult::Save,
            Key::Esc => return PopupResult::Cancel,
            _ => {}
        }
        PopupResult::Continue
    }

    fn adjust(&mut self, delta: i16) {
        let item = &mut self.items[self.selected];
        if item.kind != ItemKind::Volume { return; }
        let new_val = (item.value as i16 + delta).clamp(0, 100) as u8;
        item.value = new_val;
    }

    // ── rendering ─────────────────────────────────────────────────────────────

    pub fn draw(&self, f: &mut Frame) {
        let area = popup_area(f.area(), 55, 60);
        f.render_widget(Clear, area);

        let block = Block::default()
            .title(" 🔊 Volume & Settings  [Enter=Save  Esc=Cancel] ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        f.render_widget(block.clone(), area);

        let inner = block.inner(area);
        // header + one row per item + footer hint
        let rows = self.items.len() as u16;
        let mut constraints = vec![Constraint::Length(1)]; // header
        for _ in 0..rows { constraints.push(Constraint::Length(2)); }
        constraints.push(Constraint::Length(1)); // footer
        constraints.push(Constraint::Min(0));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(inner);

        // header
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  ←/→ adjust   Space=toggle   ↑/↓ navigate", Style::default().fg(Color::DarkGray)),
            ])),
            chunks[0],
        );

        for (i, item) in self.items.iter().enumerate() {
            let is_sel = i == self.selected;
            let row_area = chunks[i + 1];

            let row_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(16),
                    Constraint::Length(24),
                    Constraint::Min(0),
                ])
                .split(row_area);

            // label
            let label_style = if is_sel {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if is_sel { "▶ " } else { "  " };
            f.render_widget(
                Paragraph::new(format!("{}{}", prefix, item.label)).style(label_style),
                row_chunks[0],
            );

            // value widget
            let value_widget = match item.kind {
                ItemKind::Volume => Self::render_bar(item.value, is_sel),
                ItemKind::Toggle => Self::render_toggle(item.is_on(), is_sel),
            };
            f.render_widget(value_widget, row_chunks[1]);

            // hint
            f.render_widget(
                Paragraph::new(
                    Span::styled(format!("  {}", item.hint), Style::default().fg(Color::DarkGray)),
                ),
                row_chunks[2],
            );
        }

        // footer
        let footer_idx = self.items.len() + 1;
        if footer_idx < chunks.len() {
            f.render_widget(
                Paragraph::new(
                    Span::styled("  Note: TTS queue volume changes take effect after restart", Style::default().fg(Color::DarkGray)),
                ),
                chunks[footer_idx],
            );
        }
    }

    fn render_bar(value: u8, selected: bool) -> Paragraph<'static> {
        let filled = (value as usize * BAR_WIDTH / 100).min(BAR_WIDTH);
        let empty = BAR_WIDTH - filled;
        let bar: String = format!("[{}{}] {:>3}%",
            "▓".repeat(filled),
            "░".repeat(empty),
            value,
        );
        let color = if selected { Color::Yellow } else { Color::Green };
        Paragraph::new(Span::styled(bar, Style::default().fg(color)))
    }

    fn render_toggle(on: bool, selected: bool) -> Paragraph<'static> {
        let (text, color) = if on {
            ("  ● ON ", Color::Green)
        } else {
            ("  ○ OFF", Color::Red)
        };
        let style = if selected {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        Paragraph::new(Span::styled(text, style))
    }
}
