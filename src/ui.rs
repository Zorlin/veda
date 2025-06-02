use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};
use chrono::Local;
use crate::app_state::{AppState, ClaudeInstance, Message, TodoItem};

pub fn draw_ui(f: &mut ratatui::Frame, app: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Main content
        ])
        .split(f.area());

    // Render tab bar
    render_tab_bar(f, app, chunks[0]);
    
    // Render main content
    render_main_content(f, app, chunks[1]);
}

fn render_tab_bar(f: &mut ratatui::Frame, app: &AppState, area: Rect) {
    let titles: Vec<String> = app.instances.iter().enumerate().map(|(i, instance)| {
        let name = &instance.name;
        let indicator = match instance.state {
            crate::app_state::SliceState::Available => "●",
            crate::app_state::SliceState::WorkingOnTask => "⚡",
            crate::app_state::SliceState::SpawningInstances => "🔄",
            crate::app_state::SliceState::BackgroundWork => "🔧",
        };
        
        let session_indicator = if instance.session_id.is_some() { "🔗" } else { "" };
        let processing_indicator = if instance.is_processing { "⏳" } else { "" };
        
        format!("{} {} {}{}{}", 
            i, 
            name, 
            indicator,
            session_indicator,
            processing_indicator
        )
    }).collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Veda Slices"))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .select(app.current_tab);
    
    f.render_widget(tabs, area);
}

fn render_main_content(f: &mut ratatui::Frame, app: &mut AppState, area: Rect) {
    if app.show_help {
        render_help_screen(f, app, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Messages area
            Constraint::Length(6), // Input area
        ])
        .split(area);

    // Render messages with scrollable area
    render_messages_scrollable(f, app, chunks[0]);
    
    // Render input area
    let instance = &app.instances[app.current_tab];
    render_input_area(f, app, instance, chunks[1]);
    
    // Render todo list if visible
    if app.todo_state.visible {
        render_todo_overlay(f, app, area);
    }
}

fn render_messages_scrollable(f: &mut ratatui::Frame, app: &mut AppState, area: Rect) {
    let instance = &mut app.instances[app.current_tab];
    
    // Sync messages to scrollable area if needed
    instance.sync_scrollable_messages();
    
    let current_dir = format_working_directory(&instance.working_directory);
    
    let title = format!(
        "Messages - {} [Auto: {}] [CoT: {}] [Coord: {}] [Dir: {}]{}",
        instance.name,
        if app.global_state.auto_mode { "ON" } else { "OFF" },
        "OFF", // TODO: Add chain of thought toggle
        if app.global_state.coordination_mode { "ON" } else { "OFF" },
        current_dir,
        format_session_id(&instance.session_id)
    );
    
    // Use the scrollable text area to render messages
    instance.scrollable_messages.render(f, area, &title);
}

fn render_messages_old(f: &mut ratatui::Frame, app: &AppState, instance: &ClaudeInstance, area: Rect) {
    let mut all_lines = Vec::new();
    
    // Create lines for each message
    for (actual_idx, msg) in instance.messages.iter().enumerate() {
        let mut content = vec![
            Span::styled(
                format!("[{}] ", msg.timestamp),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                &msg.sender,
                match msg.sender.as_str() {
                    "You" => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    "Claude" => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    "System" => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    "DeepSeek" => Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                    _ => Style::default().fg(Color::White),
                },
            ),
            Span::raw(": "),
        ];
        
        // Handle DeepSeek thinking messages
        if msg.sender == "DeepSeek" && msg.is_thinking {
            if msg.is_collapsed {
                content.push(Span::styled(
                    "[🤔 Chain of Thought - Click to expand]",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                ));
            } else {
                content.push(Span::styled(
                    &msg.content,
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                ));
            }
        } else if msg.is_tool_use {
            // Handle tool usage messages with special formatting
            content.push(Span::styled(
                &msg.content,
                Style::default().fg(Color::Blue).add_modifier(Modifier::ITALIC),
            ));
            
            // Add tool details if available
            for tool_call in &msg.tool_calls {
                content.push(Span::raw("\n    "));
                let tool_icon = match tool_call.tool_name.as_str() {
                    name if name.contains("deepwiki") || name.contains("wiki") => "📚",
                    name if name.contains("taskmaster") => "📋",
                    name if name.contains("playwright") => "🎭",
                    name if name.contains("bash") || name.contains("command") => "💻",
                    name if name.contains("read") || name.contains("file") => "📖",
                    name if name.contains("edit") || name.contains("write") => "✏️",
                    _ => "🔧",
                };
                
                content.push(Span::styled(
                    format!("{} {}: ", tool_icon, tool_call.tool_name),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ));
                
                let truncated_params = if tool_call.parameters.len() > 80 {
                    format!("{}...", &tool_call.parameters[..80])
                } else {
                    tool_call.parameters.clone()
                };
                
                content.push(Span::styled(
                    truncated_params,
                    Style::default().fg(Color::DarkGray),
                ));
                
                // Show result status
                let status_style = match &tool_call.status {
                    crate::app_state::ToolCallStatus::Requested => Style::default().fg(Color::Yellow),
                    crate::app_state::ToolCallStatus::InProgress => Style::default().fg(Color::Blue),
                    crate::app_state::ToolCallStatus::Completed => Style::default().fg(Color::Green),
                    crate::app_state::ToolCallStatus::Failed(_) => Style::default().fg(Color::Red),
                };
                
                let status_text = match &tool_call.status {
                    crate::app_state::ToolCallStatus::Requested => " [Requested]",
                    crate::app_state::ToolCallStatus::InProgress => " [In Progress]",
                    crate::app_state::ToolCallStatus::Completed => " [✓ Complete]",
                    crate::app_state::ToolCallStatus::Failed(_) => " [✗ Failed]",
                };
                
                content.push(Span::styled(status_text, status_style));
            }
        } else {
            // Sanitize content to prevent terminal issues
            let safe_content = msg.content
                .chars()
                .map(|c| if c.is_control() && c != '\n' && c != '\t' { '?' } else { c })
                .collect::<String>();
            content.push(Span::raw(safe_content));
        }
        
        // Apply selection highlighting
        let mut style = Style::default();
        if let (Some(start), Some(end)) = (instance.selection_start, instance.selection_end) {
            let line_y = actual_idx as u16;
            let start_y = start.1.min(end.1);
            let end_y = start.1.max(end.1);
            
            if line_y >= start_y && line_y <= end_y {
                style = style.add_modifier(Modifier::REVERSED);
            }
        }
        
        // Create line with error recovery
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Line::from(content).style(style)
        })) {
            Ok(line) => {
                all_lines.push(line);
                all_lines.push(Line::from("")); // Empty line for readability
            }
            Err(e) => {
                tracing::error!("Failed to render message line {}: {:?}", actual_idx, e);
                all_lines.push(Line::from(format!("[Error rendering message {}]", actual_idx)));
                all_lines.push(Line::from(""));
            }
        }
    }
    
    let current_dir = format_working_directory(&instance.working_directory);
    
    let messages_paragraph = Paragraph::new(all_lines)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Messages - {} [Auto: {}] [CoT: {}] [Coord: {}] [Dir: {}]{}",
            instance.name,
            if app.global_state.auto_mode { "ON" } else { "OFF" },
            "OFF", // TODO: Add chain of thought toggle
            if app.global_state.coordination_mode { "ON" } else { "OFF" },
            current_dir,
            format_session_id(&instance.session_id)
        )))
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false })
        .scroll((instance.scroll_offset, 0));
    
    f.render_widget(messages_paragraph, area);
}

fn render_input_area(f: &mut ratatui::Frame, app: &AppState, instance: &ClaudeInstance, area: Rect) {
    let title = if instance.is_processing {
        "Input (Processing...)".to_string()
    } else {
        "Input (Enter to send, Shift+Enter for new line, 3x Enter to interrupt)".to_string()
    };
    
    let mut textarea = instance.textarea.clone();
    textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(if instance.is_processing {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Green)
            })
    );
    
    f.render_widget(&textarea, area);
}

fn render_todo_overlay(f: &mut ratatui::Frame, app: &AppState, area: Rect) {
    let block = Block::default()
        .title("Todo List (Press 't' to toggle)")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black));
    
    let popup_area = centered_rect(60, 80, area);
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);

    let inner = popup_area.inner(ratatui::layout::Margin { vertical: 1, horizontal: 1 });
    
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("Last updated: {}", app.todo_state.last_update.format("%H:%M:%S")),
        Style::default().fg(Color::Gray)
    )));
    lines.push(Line::from(""));

    for (i, item) in app.todo_state.items.iter().enumerate() {
        let status_color = match item.status.as_str() {
            "completed" => Color::Green,
            "in_progress" => Color::Yellow,
            _ => Color::White,
        };
        
        let priority_icon = match item.priority.as_str() {
            "high" => "🔴",
            "medium" => "🟡", 
            "low" => "🟢",
            _ => "⚪",
        };
        
        lines.push(Line::from(vec![
            Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::DarkGray)),
            Span::raw(priority_icon),
            Span::raw(" "),
            Span::styled(&item.content, Style::default().fg(status_color)),
            Span::styled(format!(" [{}]", item.status), Style::default().fg(Color::DarkGray)),
        ]));
    }

    if app.todo_state.items.is_empty() {
        lines.push(Line::from(Span::styled(
            "No todos available",
            Style::default().fg(Color::DarkGray)
        )));
    }

    let todo_paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: true });
    
    f.render_widget(todo_paragraph, inner);
}

fn render_help_screen(f: &mut ratatui::Frame, app: &AppState, area: Rect) {
    let help_content = vec![
        Line::from(Span::styled("Veda - Claude Multi-Instance Manager", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled("Navigation:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::from("  Tab/Shift+Tab - Switch between Claude instances"),
        Line::from("  Ctrl+N - Create new instance"),
        Line::from("  Ctrl+W - Close current instance (if not last)"),
        Line::from("  Ctrl+D - Toggle debug mode"),
        Line::from(""),
        Line::from(Span::styled("Messages:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::from("  Enter - Send message"),
        Line::from("  Shift+Enter - New line in input"),
        Line::from("  3x Enter quickly - Interrupt Claude"),
        Line::from("  Ctrl+C - Copy selected text"),
        Line::from("  PageUp/PageDown - Scroll messages"),
        Line::from(""),
        Line::from(Span::styled("Features:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::from("  Ctrl+A - Toggle auto mode"),
        Line::from("  Ctrl+Shift+C - Toggle coordination mode"),
        Line::from("  t - Toggle todo list"),
        Line::from("  F5 - Refresh todo list"),
        Line::from(""),
        Line::from(Span::styled("Advanced:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::from("  Ctrl+S - Spawn additional instances"),
        Line::from("  Mouse selection for copying text"),
        Line::from(""),
        Line::from("Press '?' or 'h' to close this help"),
    ];

    let help_paragraph = Paragraph::new(help_content)
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .wrap(Wrap { trim: true })
        .scroll((app.help_scroll, 0));
    
    f.render_widget(help_paragraph, area);
}

// Helper functions
fn format_working_directory(dir: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if dir.starts_with(&home) {
            dir.replacen(&home, "~", 1)
        } else {
            dir.to_string()
        }
    } else {
        dir.to_string()
    }
}

fn format_session_id(session_id: &Option<String>) -> String {
    if let Some(ref sid) = session_id {
        let display_len = 8.min(sid.len());
        let start = sid.len().saturating_sub(display_len);
        format!(" [Session: ...{}]", &sid[start..])
    } else {
        String::new()
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}