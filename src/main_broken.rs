mod claude;
mod deepseek;
mod shared_ipc;
mod ui_components;
mod app_state;
mod ui;
mod events;
mod stall_detection;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind, EnableBracketedPaste, DisableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::{
    io,
    time::Duration,
};
use tokio::time::timeout;

use crate::app_state::AppState;
use crate::ui::draw_ui;
use crate::events::{handle_key_event, handle_mouse_event};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = AppState::new();

    // Main event loop
    let result = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut AppState,
) -> Result<()> {
    loop {
        // Draw UI
        terminal.draw(|f| draw_ui(f, app))?;

        // Handle events with timeout
        let event_result = timeout(Duration::from_millis(100), async {
            event::read()
        }).await;
        
        match event_result {
            Ok(Ok(Event::Key(key))) => {
                if handle_key_event(app, key.code, key.modifiers).await? {
                    break;
                }
            }
            Ok(Ok(Event::Mouse(mouse))) => {
                handle_mouse_event(app, mouse.kind, mouse.column, mouse.row, mouse.modifiers)?;
            }
            Ok(Ok(Event::Resize(_, _))) => {
                // Terminal was resized, no action needed
            }
            Ok(Ok(_)) => {
                // Other events, ignore
            }
            Ok(Err(e)) => {
                tracing::error!("Event error: {}", e);
            }
            Err(_) => {
                // Timeout, check for background tasks
                handle_background_tasks(app).await?;
            }
        }

        // Process any pending messages or state updates
        update_app_state(app).await?;
    }

    Ok(())
}

async fn handle_background_tasks(app: &mut AppState) -> Result<()> {
    // Check for stall detection
    for instance in &mut app.instances {
        if instance.should_check_for_stall() {
            // Trigger stall intervention
            tracing::info!("Triggering stall intervention for instance {}", instance.name);
            instance.stall_check_sent = true;
            instance.stall_intervention_in_progress = true;
            
            // Here you would implement the actual stall intervention logic
            // For now, just add a system message
            let message = app_state::Message::system(
                "Conversation stalled - analyzing for next steps...".to_string()
            );
            instance.messages.push(message);
        }
    }

    // Handle DeepSeek streaming
    for instance in &mut app.instances {
        let mut should_clear_receiver = false;
        let mut error_message = None;
        
        if let Some(ref mut receiver) = instance.deepseek_receiver {
            while let Ok(msg) = receiver.try_recv() {
                match msg {
                    deepseek::DeepSeekMessage::Start { is_thinking } => {
                        if instance.deepseek_accumulated_text.is_empty() {
                            let message = app_state::Message::deepseek(String::new(), is_thinking);
                            instance.messages.push(message);
                        }
                    }
                    deepseek::DeepSeekMessage::Text { text, is_thinking } => {
                        instance.deepseek_accumulated_text.push_str(&text);
                        if let Some(last_msg) = instance.messages.last_mut() {
                            if last_msg.sender == "DeepSeek" {
                                last_msg.content = instance.deepseek_accumulated_text.clone();
                                last_msg.is_thinking = is_thinking;
                            }
                        }
                    }
                    deepseek::DeepSeekMessage::End => {
                        instance.deepseek_streaming = false;
                        should_clear_receiver = true;
                        instance.deepseek_accumulated_text.clear();
                    }
                    deepseek::DeepSeekMessage::Error { error } => {
                        error_message = Some(error);
                        instance.deepseek_streaming = false;
                        should_clear_receiver = true;
                        instance.deepseek_accumulated_text.clear();
                    }
                }
            }
        }
        
        if should_clear_receiver {
            instance.deepseek_receiver = None;
        }
        
        if let Some(error) = error_message {
            let message = app_state::Message::system(format!("DeepSeek error: {}", error));
            instance.messages.push(message);
        }
    }

    Ok(())
}

async fn update_app_state(app: &mut AppState) -> Result<()> {
    // Update coordination state
    if app.global_state.coordination_mode {
        // Check if coordination updates are needed
        let now = chrono::Local::now();
        let time_since_update = now.signed_duration_since(app.global_state.last_coordination_update);
        
        if time_since_update.num_seconds() > 30 {
            app.global_state.last_coordination_update = now;
            // Perform coordination logic here
        }
    }

    // Update slice states based on activity
    for instance in &mut app.instances {
        // Update slice state based on current activity
        if instance.is_processing {
            if instance.state == app_state::SliceState::Available {
                instance.state = app_state::SliceState::WorkingOnTask;
            }
        } else if instance.state == app_state::SliceState::WorkingOnTask {
            instance.state = app_state::SliceState::Available;
        }
    }

    Ok(())
}