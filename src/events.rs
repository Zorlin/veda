use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};
use arboard::Clipboard;
use anyhow::Result;
use chrono::Local;
use crate::app_state::AppState;

pub async fn handle_key_event(
    app: &mut AppState, 
    key_code: KeyCode, 
    modifiers: KeyModifiers,
) -> Result<bool> {
    // Handle help screen
    if app.show_help {
        match key_code {
            KeyCode::Char('?') | KeyCode::Char('h') => {
                app.show_help = false;
                return Ok(false);
            }
            KeyCode::PageUp => {
                app.help_scroll = app.help_scroll.saturating_sub(5);
                return Ok(false);
            }
            KeyCode::PageDown => {
                app.help_scroll = app.help_scroll.saturating_add(5);
                return Ok(false);
            }
            _ => return Ok(false),
        }
    }

    match (key_code, modifiers) {
        // Help
        (KeyCode::Char('?'), KeyModifiers::NONE) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
            app.show_help = !app.show_help;
            Ok(false)
        }

        // Navigation
        (KeyCode::Tab, KeyModifiers::NONE) => {
            app.next_tab();
            Ok(false)
        }
        (KeyCode::BackTab, KeyModifiers::SHIFT) => {
            app.previous_tab();
            Ok(false)
        }

        // Instance management
        (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            let new_index = app.instances.len();
            let name = format!("Slice {}", new_index);
            app.add_instance(name);
            app.current_tab = new_index;
            Ok(false)
        }
        (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
            if app.instances.len() > 1 {
                app.remove_instance(app.current_tab);
            }
            Ok(false)
        }

        // Mode toggles
        (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
            app.global_state.auto_mode = !app.global_state.auto_mode;
            let status = if app.global_state.auto_mode { "enabled" } else { "disabled" };
            let message = crate::app_state::Message::system(format!("Auto mode {}", status));
            app.current_instance_mut().add_message(message);
            Ok(false)
        }
        // Coordination mode toggle with Ctrl+R (for "coordination")
        (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
            app.global_state.coordination_mode = !app.global_state.coordination_mode;
            let status = if app.global_state.coordination_mode { "enabled" } else { "disabled" };
            let message = crate::app_state::Message::system(format!("Coordination mode {}", status));
            app.current_instance_mut().add_message(message);
            Ok(false)
        }

        // Debug mode
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
            app.debug_mode = !app.debug_mode;
            let status = if app.debug_mode { "enabled" } else { "disabled" };
            let message = crate::app_state::Message::system(format!("Debug mode {}", status));
            app.current_instance_mut().add_message(message);
            Ok(false)
        }

        // Todo list
        (KeyCode::Char('t'), KeyModifiers::NONE) => {
            app.todo_state.visible = !app.todo_state.visible;
            Ok(false)
        }
        (KeyCode::F(5), KeyModifiers::NONE) => {
            // Refresh todo list (placeholder)
            app.todo_state.last_update = Local::now();
            Ok(false)
        }

        // Scrolling
        (KeyCode::PageUp, KeyModifiers::NONE) => {
            let instance = app.current_instance_mut();
            instance.scrollable_messages.scroll_up(5);
            instance.scroll_offset = instance.scroll_offset.saturating_sub(5);
            Ok(false)
        }
        (KeyCode::PageDown, KeyModifiers::NONE) => {
            let instance = app.current_instance_mut();
            instance.scrollable_messages.scroll_down(5);
            instance.scroll_offset = instance.scroll_offset.saturating_add(5);
            Ok(false)
        }
        // Only handle scroll keys when NOT in help mode and NOT focused on input
        (KeyCode::Up, KeyModifiers::CONTROL) => {
            let instance = app.current_instance_mut();
            instance.scrollable_messages.scroll_up(1);
            Ok(false)
        }
        (KeyCode::Down, KeyModifiers::CONTROL) => {
            let instance = app.current_instance_mut();
            instance.scrollable_messages.scroll_down(1);
            Ok(false)
        }
        (KeyCode::Home, KeyModifiers::CONTROL) => {
            let instance = app.current_instance_mut();
            instance.scrollable_messages.scroll_to_top();
            Ok(false)
        }
        (KeyCode::End, KeyModifiers::CONTROL) => {
            let instance = app.current_instance_mut();
            instance.scrollable_messages.scroll_to_bottom();
            Ok(false)
        }

        // Copy selected text with Ctrl+Shift+C to avoid conflicts
        (KeyCode::Char('c'), KeyModifiers::CONTROL | KeyModifiers::SHIFT) => {
            handle_copy_selection(app)?;
            Ok(false)
        }

        // Send message or interrupt
        (KeyCode::Enter, KeyModifiers::NONE) => {
            if should_interrupt(app) {
                handle_interrupt(app);
                Ok(false)
            } else {
                handle_send_message(app).await
            }
        }

        // Spawn instances
        (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
            handle_spawn_instances(app).await;
            Ok(false)
        }

        // Exit
        (KeyCode::Char('q'), KeyModifiers::CONTROL) => Ok(true),

        // Pass other keys to textarea
        _ => {
            let instance = app.current_instance_mut();
            instance.textarea.input(Event::Key(crossterm::event::KeyEvent::new(key_code, modifiers)));
            Ok(false)
        }
    }
}

pub fn handle_mouse_event(
    app: &mut AppState,
    kind: MouseEventKind,
    column: u16,
    row: u16,
    _modifiers: KeyModifiers,
) -> Result<bool> {
    match kind {
        MouseEventKind::Down(_button) => {
            let instance = app.current_instance_mut();
            instance.selecting = true;
            instance.selection_start = Some((column, row));
            instance.selection_end = Some((column, row));
            Ok(false)
        }
        MouseEventKind::Drag(_button) => {
            let instance = app.current_instance_mut();
            if instance.selecting {
                instance.selection_end = Some((column, row));
            }
            Ok(false)
        }
        MouseEventKind::Up(_button) => {
            let instance = app.current_instance_mut();
            instance.selecting = false;
            Ok(false)
        }
        MouseEventKind::ScrollUp => {
            let instance = app.current_instance_mut();
            instance.scrollable_messages.scroll_up(3);
            instance.scroll_offset = instance.scroll_offset.saturating_sub(3);
            Ok(false)
        }
        MouseEventKind::ScrollDown => {
            let instance = app.current_instance_mut();
            instance.scrollable_messages.scroll_down(3);
            instance.scroll_offset = instance.scroll_offset.saturating_add(3);
            Ok(false)
        }
        _ => Ok(false),
    }
}

// Helper functions
fn handle_copy_selection(app: &mut AppState) -> Result<()> {
    let instance = &app.instances[app.current_tab];
    
    if let (Some(start), Some(end)) = (instance.selection_start, instance.selection_end) {
        let start_y = start.1.min(end.1) as usize;
        let end_y = start.1.max(end.1) as usize;
        
        let mut selected_text = String::new();
        for (i, msg) in instance.messages.iter().enumerate() {
            if i >= start_y && i <= end_y {
                if !selected_text.is_empty() {
                    selected_text.push('\n');
                }
                selected_text.push_str(&format!("[{}] {}: {}", 
                    msg.timestamp, msg.sender, msg.content));
            }
        }
        
        if !selected_text.is_empty() {
            let mut clipboard = Clipboard::new()?;
            clipboard.set_text(selected_text)?;
            
            let message = crate::app_state::Message::system("Selected text copied to clipboard".to_string());
            app.current_instance_mut().add_message(message);
        }
    }
    
    Ok(())
}

async fn handle_send_message(app: &mut AppState) -> Result<bool> {
    let instance = app.current_instance_mut();
    
    if instance.is_processing {
        return Ok(false);
    }
    
    let input = instance.textarea.lines().join("\n").trim().to_string();
    if input.is_empty() {
        return Ok(false);
    }
    
    // Clear the textarea
    instance.textarea.select_all();
    instance.textarea.cut();
    
    // Add user message
    let user_message = crate::app_state::Message::new("You".to_string(), input.clone());
    instance.add_message(user_message);
    
    // Update activity tracking
    instance.last_activity = Local::now();
    instance.is_processing = true;
    
    // Here you would send to Claude (placeholder)
    // send_to_claude(instance, input).await?;
    
    Ok(false)
}

fn should_interrupt(app: &AppState) -> bool {
    // Implement triple-enter detection logic
    // This is a placeholder - you'd track recent enter presses
    false
}

fn handle_interrupt(app: &mut AppState) {
    let instance = app.current_instance_mut();
    if instance.is_processing {
        instance.is_processing = false;
        let message = crate::app_state::Message::system("Processing interrupted by user".to_string());
        instance.add_message(message);
    }
}

async fn handle_spawn_instances(app: &mut AppState) {
    // Placeholder for spawning additional instances
    let current_instance = app.current_instance_mut();
    current_instance.state = crate::app_state::SliceState::SpawningInstances;
    
    let message = crate::app_state::Message::system("Spawning additional instances for parallel work...".to_string());
    current_instance.add_message(message);
    
    // Here you would implement the actual spawning logic
}