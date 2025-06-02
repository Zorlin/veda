use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

/// A scrollable text area with scrollbar for displaying chat messages
pub struct ScrollableTextArea {
    pub content: Vec<String>,
    pub scroll_position: usize,
    pub max_scroll: usize,
}

impl ScrollableTextArea {
    pub fn new() -> Self {
        Self {
            content: Vec::new(),
            scroll_position: 0,
            max_scroll: 0,
        }
    }

    pub fn add_message(&mut self, message: String) {
        self.content.push(message);
        self.update_max_scroll();
        // Auto-scroll to bottom when new message is added
        self.scroll_to_bottom();
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_position = self.scroll_position.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_position = (self.scroll_position + lines).min(self.max_scroll);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_position = self.max_scroll;
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_position = 0;
    }

    fn update_max_scroll(&mut self) {
        // Calculate based on content height vs display area
        // This is a simplified calculation - you'd want to consider word wrapping
        self.max_scroll = self.content.len().saturating_sub(1);
    }

    pub fn render(&mut self, f: &mut ratatui::Frame, area: Rect, title: &str) {
        // Split area to leave space for scrollbar
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);

        // Prepare content as Text
        let mut lines = Vec::new();
        for (i, message) in self.content.iter().enumerate() {
            // Add enhanced styling based on message content
            let style = if message.contains("🔧 Using tool:") {
                Style::default().fg(Color::Blue).add_modifier(Modifier::ITALIC)
            } else if message.contains("System:") {
                Style::default().fg(Color::Yellow)
            } else if message.contains("Claude:") {
                Style::default().fg(Color::Green)
            } else if message.contains("You:") {
                Style::default().fg(Color::Cyan)
            } else if message.contains("DeepSeek:") {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default().fg(Color::White)
            };

            // Special handling for tool usage messages
            if message.contains("🔧 Using tool:") {
                // Split the message to show tool details nicely
                let parts: Vec<&str> = message.split(" with parameters: ").collect();
                if parts.len() == 2 {
                    let mut line_content = Vec::new();
                    line_content.push(Span::styled(parts[0], Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)));
                    line_content.push(Span::styled(" with parameters: ", Style::default().fg(Color::DarkGray)));
                    line_content.push(Span::styled(parts[1], Style::default().fg(Color::DarkGray)));
                    lines.push(Line::from(line_content));
                } else {
                    lines.push(Line::from(vec![Span::styled(message.clone(), style)]));
                }
            } else {
                lines.push(Line::from(vec![Span::styled(message.clone(), style)]));
            }
        }

        let text = Text::from(lines);

        // Create paragraph with scrolling
        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_style(Style::default().fg(Color::Blue)),
            )
            .wrap(Wrap { trim: true })
            .scroll((self.scroll_position as u16, 0));

        f.render_widget(paragraph, chunks[0]);

        // Render scrollbar
        if self.max_scroll > 0 {
            let mut scrollbar_state = ScrollbarState::new(self.max_scroll)
                .position(self.scroll_position);

            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"))
                .track_symbol(Some("│"))
                .thumb_symbol("█");

            f.render_stateful_widget(scrollbar, chunks[1], &mut scrollbar_state);
        }
    }

    pub fn handle_scroll_event(&mut self, direction: ScrollDirection, amount: usize) {
        match direction {
            ScrollDirection::Up => self.scroll_up(amount),
            ScrollDirection::Down => self.scroll_down(amount),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ScrollDirection {
    Up,
    Down,
}

impl Default for ScrollableTextArea {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrollable_text_area() {
        let mut area = ScrollableTextArea::new();
        
        area.add_message("First message".to_string());
        area.add_message("Second message".to_string());
        
        assert_eq!(area.content.len(), 2);
        assert_eq!(area.scroll_position, area.max_scroll);
        
        area.scroll_up(1);
        assert!(area.scroll_position < area.max_scroll);
        
        area.scroll_to_bottom();
        assert_eq!(area.scroll_position, area.max_scroll);
    }
}