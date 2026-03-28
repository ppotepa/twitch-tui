use std::collections::HashMap;

use tui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::SharedMessages,
    ui::components::{Component, utils::popup_area},
};

static STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "is", "it",
    "i", "you", "me", "my", "your", "we", "he", "she", "they", "this", "that", "was", "are", "be",
    "been", "have", "has", "had", "do", "did", "will", "not", "no", "so", "if", "as", "up", "by",
    "from", "just", "like", "what",
];

pub struct ChatStatsWidget {
    pub visible: bool,
    messages: SharedMessages,
}

impl ChatStatsWidget {
    pub const fn new(messages: SharedMessages) -> Self {
        Self {
            visible: false,
            messages,
        }
    }

    pub const fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    fn compute(&self) -> Vec<Line<'static>> {
        let msgs = self.messages.borrow();
        let total: usize = msgs.len();
        let non_system: Vec<_> = msgs.iter().filter(|m| !m.system).collect();
        let total_chat = non_system.len();

        let mut chatter_counts: HashMap<String, usize> = HashMap::new();
        let mut word_counts: HashMap<String, usize> = HashMap::new();

        for m in &non_system {
            *chatter_counts.entry(m.author.clone()).or_default() += 1;
            for word in m.payload.split_whitespace() {
                let w = word.to_lowercase();
                let clean: String = w.chars().filter(|c| c.is_alphabetic()).collect();
                if clean.len() > 2 && !STOP_WORDS.contains(&clean.as_str()) {
                    *word_counts.entry(clean).or_default() += 1;
                }
            }
        }

        let unique_chatters = chatter_counts.len();

        let mut top_chatters: Vec<(String, usize)> = chatter_counts.into_iter().collect();
        top_chatters.sort_by(|a, b| b.1.cmp(&a.1));

        let mut top_words: Vec<(String, usize)> = word_counts.into_iter().collect();
        top_words.sort_by(|a, b| b.1.cmp(&a.1));

        let header_style = Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Cyan);
        let val_style = Style::default().fg(Color::White);
        let dim_style = Style::default().fg(Color::DarkGray);

        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled("─── Chat Statistics ───", header_style)),
            Line::default(),
            Line::from(vec![
                Span::styled("Total messages:  ", dim_style),
                Span::styled(total.to_string(), val_style),
            ]),
            Line::from(vec![
                Span::styled("Chat messages:   ", dim_style),
                Span::styled(total_chat.to_string(), val_style),
            ]),
            Line::from(vec![
                Span::styled("Unique chatters: ", dim_style),
                Span::styled(unique_chatters.to_string(), val_style),
            ]),
            Line::default(),
            Line::from(Span::styled("Top chatters:", header_style)),
        ];

        for (i, (name, count)) in top_chatters.iter().take(5).enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {}. ", i + 1), dim_style),
                Span::styled(format!("{name:<20}"), val_style),
                Span::styled(format!(" {count} msgs"), Style::default().fg(Color::Yellow)),
            ]));
        }

        lines.push(Line::default());
        lines.push(Line::from(Span::styled("Top words:", header_style)));
        for (i, (word, count)) in top_words.iter().take(5).enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {}. ", i + 1), dim_style),
                Span::styled(format!("{word:<20}"), val_style),
                Span::styled(format!(" ×{count}"), Style::default().fg(Color::Green)),
            ]));
        }

        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Press m to close",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));

        lines
    }
}

impl Component for ChatStatsWidget {
    fn draw(&mut self, f: &mut Frame, area: Option<Rect>) {
        if !self.visible {
            return;
        }

        let area = popup_area(area.unwrap_or_else(|| f.area()), 50, 70);
        let lines = self.compute();
        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 📊 Chat Stats ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        f.render_widget(Clear, area);
        f.render_widget(paragraph, area);
    }
}
