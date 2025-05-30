mod claude;
mod deepseek;

use anyhow::Result;
use arboard::Clipboard;
use chrono::{Local, DateTime};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind, EnableBracketedPaste, DisableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame, Terminal,
};
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;
use uuid::Uuid;
use serde_json;
use tui_textarea::TextArea;

use crate::claude::{ClaudeMessage, send_to_claude_with_session, enable_claude_tool};
use crate::deepseek::{analyze_claude_message, generate_deepseek_response_stream, 
                      check_tool_permission_issue, DeepSeekMessage};

#[derive(Debug, Clone)]
struct Message {
    timestamp: String,
    sender: String,
    content: String,
    // For DeepSeek messages
    is_thinking: bool,
    is_collapsed: bool,
}

#[derive(Debug, Clone)]
struct TodoItem {
    id: String,
    content: String,
    status: String,
    priority: String,
}

#[derive(Debug)]
struct TodoListState {
    items: Vec<TodoItem>,
    visible: bool,
    last_update: DateTime<Local>,
}

struct ClaudeInstance {
    id: Uuid,
    name: String,
    messages: Vec<Message>,
    textarea: TextArea<'static>,
    is_processing: bool,
    // Text selection state
    selection_start: Option<(u16, u16)>,
    selection_end: Option<(u16, u16)>,
    selecting: bool,
    scroll_offset: u16,
    // Track tool use attempts
    last_tool_attempts: Vec<String>,
    // Claude session ID for resume
    session_id: Option<String>,
}

impl ClaudeInstance {
    fn new(name: String) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Input")
        );
        // Start with single line, will expand as needed
        textarea.set_max_histories(100);
        
        Self {
            id: Uuid::new_v4(),
            name,
            messages: Vec::new(),
            textarea,
            is_processing: false,
            selection_start: None,
            selection_end: None,
            selecting: false,
            scroll_offset: 0,
            last_tool_attempts: Vec::new(),
            session_id: None,
        }
    }

    fn add_message(&mut self, sender: String, content: String) {
        self.add_message_with_flags(sender, content, false, false);
    }
    
    fn add_message_with_flags(&mut self, sender: String, content: String, is_thinking: bool, is_collapsed: bool) {
        let timestamp = Local::now().format("%H:%M:%S").to_string();
        self.messages.push(Message {
            timestamp,
            sender,
            content,
            is_thinking,
            is_collapsed,
        });
    }

    fn get_selected_text(&self) -> Option<String> {
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let mut selected_lines = Vec::new();
            let start_y = start.1.min(end.1) as usize;
            let end_y = start.1.max(end.1) as usize;
            
            for (i, msg) in self.messages.iter().enumerate() {
                if i >= start_y && i <= end_y {
                    selected_lines.push(format!("{} {}: {}", msg.timestamp, msg.sender, msg.content));
                }
            }
            
            if !selected_lines.is_empty() {
                return Some(selected_lines.join("\n"));
            }
        }
        None
    }
}

struct App {
    instances: Vec<ClaudeInstance>,
    current_tab: usize,
    auto_mode: bool,
    show_chain_of_thought: bool,
    clipboard: Arc<Mutex<Clipboard>>,
    message_tx: mpsc::Sender<ClaudeMessage>,
    message_rx: mpsc::Receiver<ClaudeMessage>,
    deepseek_tx: mpsc::Sender<DeepSeekMessage>,
    deepseek_rx: mpsc::Receiver<DeepSeekMessage>,
    // Collect DeepSeek responses for sending to Claude
    deepseek_response_buffer: String,
    collecting_deepseek_response: bool,
    // Todo list overlay
    todo_list: TodoListState,
}

impl App {
    fn new() -> Result<Self> {
        let mut instances = Vec::new();
        instances.push(ClaudeInstance::new("Claude 1".to_string()));
        
        let (tx, rx) = mpsc::channel(100);
        let (deepseek_tx, deepseek_rx) = mpsc::channel(100);
        
        Ok(Self {
            instances,
            current_tab: 0,
            auto_mode: true,  // Enable automode by default
            show_chain_of_thought: true,  // Show CoT by default
            clipboard: Arc::new(Mutex::new(Clipboard::new()?)),
            message_tx: tx,
            message_rx: rx,
            deepseek_tx,
            deepseek_rx,
            deepseek_response_buffer: String::new(),
            collecting_deepseek_response: false,
            todo_list: TodoListState {
                items: Vec::new(),
                visible: false,
                last_update: Local::now(),
            },
        })
    }

    fn current_instance_mut(&mut self) -> Option<&mut ClaudeInstance> {
        self.instances.get_mut(self.current_tab)
    }

    fn add_instance(&mut self) {
        let instance_num = self.instances.len() + 1;
        self.instances.push(ClaudeInstance::new(format!("Claude {}", instance_num)));
        self.current_tab = self.instances.len() - 1;
    }

    fn next_tab(&mut self) {
        if !self.instances.is_empty() {
            self.current_tab = (self.current_tab + 1) % self.instances.len();
        }
    }

    fn previous_tab(&mut self) {
        if !self.instances.is_empty() {
            self.current_tab = if self.current_tab == 0 {
                self.instances.len() - 1
            } else {
                self.current_tab - 1
            };
        }
    }

    fn toggle_auto_mode(&mut self) {
        self.auto_mode = !self.auto_mode;
    }

    fn toggle_chain_of_thought(&mut self) {
        self.show_chain_of_thought = !self.show_chain_of_thought;
    }

    fn show_todo_list(&mut self) {
        self.todo_list.visible = true;
        self.todo_list.last_update = Local::now();
    }

    fn hide_todo_list(&mut self) {
        self.todo_list.visible = false;
    }

    fn should_hide_todo_list(&self) -> bool {
        if !self.todo_list.visible {
            return false;
        }
        // Hide after 5 seconds
        let elapsed = Local::now().signed_duration_since(self.todo_list.last_update);
        elapsed.num_seconds() > 5
    }

    fn parse_todo_list(&mut self, text: &str) {
        // Try to parse JSON todo list
        if let Some(start) = text.find('[') {
            if let Some(end) = text.rfind(']') {
                let json_str = &text[start..=end];
                match serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
                    Ok(todos) => {
                        self.todo_list.items.clear();
                        for todo in todos {
                            if let Some(obj) = todo.as_object() {
                                let item = TodoItem {
                                    id: obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    content: obj.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    status: obj.get("status").and_then(|v| v.as_str()).unwrap_or("pending").to_string(),
                                    priority: obj.get("priority").and_then(|v| v.as_str()).unwrap_or("medium").to_string(),
                                };
                                self.todo_list.items.push(item);
                            }
                        }
                        self.show_todo_list();
                        tracing::info!("Parsed {} todo items", self.todo_list.items.len());
                    }
                    Err(e) => {
                        tracing::debug!("Failed to parse todo JSON: {}", e);
                    }
                }
            }
        }
    }

    async fn send_message(&mut self, message: String) {
        if let Some(instance) = self.current_instance_mut() {
            tracing::info!("Sending message to Claude instance {}: {}", instance.id, message);
            instance.add_message("You".to_string(), message.clone());
            instance.is_processing = true;
            
            let id = instance.id;
            let session_id = instance.session_id.clone();
            let tx = self.message_tx.clone();
            
            // Send to Claude
            tokio::spawn(async move {
                tracing::debug!("Spawning send_to_claude task for instance {} with session {:?}", id, session_id);
                if let Err(e) = send_to_claude_with_session(id, message.clone(), tx, session_id).await {
                    tracing::error!("Error sending to Claude: {}", e);
                    eprintln!("Error sending to Claude: {}", e);
                } else {
                    tracing::info!("Successfully initiated Claude command for message: {}", message);
                }
            });
        }
    }

    async fn process_deepseek_messages(&mut self) {
        while let Ok(msg) = self.deepseek_rx.try_recv() {
            tracing::debug!("Processing DeepSeek message: {:?}", msg);
            
            let auto_mode = self.auto_mode;
            let collecting = self.collecting_deepseek_response;
            
            match msg {
                DeepSeekMessage::Start { is_thinking } => {
                    tracing::info!("DeepSeek start, is_thinking: {}", is_thinking);
                    // Start collecting response if automode is on
                    if auto_mode {
                        self.collecting_deepseek_response = true;
                        self.deepseek_response_buffer.clear();
                    }
                    // Create a new DeepSeek message
                    if let Some(instance) = self.current_instance_mut() {
                        instance.add_message_with_flags(
                            "DeepSeek".to_string(), 
                            String::new(), 
                            is_thinking,
                            false
                        );
                    }
                }
                DeepSeekMessage::Text { text, is_thinking } => {
                    // Hide todo list when new output arrives
                    self.hide_todo_list();
                    
                    // Collect all text for processing later
                    if collecting {
                        self.deepseek_response_buffer.push_str(&text);
                    }
                    
                    // Find the last DeepSeek message to append to
                    if let Some(instance) = self.current_instance_mut() {
                        if let Some(last_msg) = instance.messages.iter_mut()
                            .rev()
                            .find(|m| m.sender == "DeepSeek") 
                        {
                            last_msg.content.push_str(&text);
                            last_msg.is_thinking = is_thinking;
                        } else {
                            // Create new message if none exists
                            instance.add_message_with_flags(
                                "DeepSeek".to_string(),
                                text,
                                is_thinking,
                                false
                            );
                        }
                    }
                }
                DeepSeekMessage::End => {
                    tracing::info!("DeepSeek response ended");
                    
                    // Send collected response to Claude if in automode
                    if self.collecting_deepseek_response && !self.deepseek_response_buffer.is_empty() {
                        self.collecting_deepseek_response = false;
                        let full_response = self.deepseek_response_buffer.trim();
                        
                        // Extract MESSAGE_TO_CLAUDE_WITH_VERDICT
                        let message_to_claude = if let Some(idx) = full_response.find("MESSAGE_TO_CLAUDE_WITH_VERDICT:") {
                            let verdict_part = &full_response[idx + "MESSAGE_TO_CLAUDE_WITH_VERDICT:".len()..];
                            verdict_part.trim().to_string()
                        } else {
                            // Fallback to full response if no verdict section found
                            full_response.to_string()
                        };
                        
                        if !message_to_claude.is_empty() {
                            if let Some(instance) = self.current_instance_mut() {
                                let instance_id = instance.id;
                                let session_id = instance.session_id.clone();
                                let tx = self.message_tx.clone();
                                
                                tokio::spawn(async move {
                                    tracing::info!("Sending DeepSeek verdict to Claude: {}", message_to_claude);
                                    if let Err(e) = send_to_claude_with_session(instance_id, message_to_claude, tx, session_id).await {
                                        tracing::error!("Failed to send DeepSeek response to Claude: {}", e);
                                    }
                                });
                            }
                        }
                    }
                }
                DeepSeekMessage::Error { error } => {
                    tracing::error!("DeepSeek error: {}", error);
                    if let Some(instance) = self.current_instance_mut() {
                        instance.add_message("DeepSeek Error".to_string(), error);
                    }
                    self.collecting_deepseek_response = false;
                }
            }
        }
    }

    async fn process_claude_messages(&mut self) {
        while let Ok(msg) = self.message_rx.try_recv() {
            tracing::debug!("Processing message: {:?}", msg);
            match msg {
                ClaudeMessage::StreamStart { instance_id } => {
                    tracing::info!("StreamStart for instance {}", instance_id);
                    // Don't create empty message - we'll create it when we get actual content
                }
                ClaudeMessage::StreamText { instance_id, text } => {
                    tracing::debug!("StreamText for instance {}: {:?}", instance_id, text);
                    // Hide todo list when new output arrives
                    self.hide_todo_list();
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        // Check if we should append to existing Claude message or create new one
                        let should_create_new = if let Some(last_msg) = instance.messages.last() {
                            // Create new message if last message was a Tool message
                            last_msg.sender == "Tool"
                        } else {
                            true // No messages yet, create new one
                        };
                        
                        if should_create_new {
                            // Create a new Claude message
                            instance.add_message("Claude".to_string(), text.clone());
                            tracing::debug!("Created new Claude message after tool use");
                            // Check if this is todo list data
                            self.parse_todo_list(&text);
                        } else {
                            // Try to append to the last Claude message
                            let needs_todo_parse = if let Some(last_msg) = instance.messages.last_mut() {
                                if last_msg.sender == "Claude" {
                                    last_msg.content.push_str(&text);
                                    tracing::debug!("Appended text to Claude message, total length: {}", last_msg.content.len());
                                    // Return the content to parse later
                                    Some(last_msg.content.clone())
                                } else {
                                    // Shouldn't happen based on our check above, but just in case
                                    instance.add_message("Claude".to_string(), text.clone());
                                    tracing::debug!("Created new Claude message");
                                    Some(text)
                                }
                            } else {
                                None
                            };
                            
                            // Parse todo list if needed (after releasing the mutable borrow)
                            if let Some(content) = needs_todo_parse {
                                self.parse_todo_list(&content);
                            }
                        }
                    } else {
                        tracing::error!("Instance {} not found", instance_id);
                    }
                }
                ClaudeMessage::StreamEnd { instance_id } => {
                    tracing::info!("StreamEnd for instance {}", instance_id);
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        instance.is_processing = false;
                        
                        // Process with automode if enabled
                        if self.auto_mode {
                            tracing::info!("Automode is ON, checking last message");
                            if let Some(last_msg) = instance.messages.last() {
                                tracing::info!("Last message sender: {}, content length: {}", last_msg.sender, last_msg.content.len());
                                if last_msg.sender == "Claude" && !last_msg.content.is_empty() {
                                    let claude_message = last_msg.content.clone();
                                    
                                    // Get user context from previous messages
                                    let user_context = instance.messages.iter()
                                        .rev()
                                        .find(|m| m.sender == "You")
                                        .map(|m| m.content.clone())
                                        .unwrap_or_default();
                                    
                                    // Check if there were recent tool attempts
                                    let had_tool_attempts = !instance.last_tool_attempts.is_empty();
                                    let attempted_tools = instance.last_tool_attempts.clone();
                                    
                                    // Clear tool attempts for next message
                                    instance.last_tool_attempts.clear();
                                    
                                    // Add system message if tools were attempted
                                    if had_tool_attempts {
                                        instance.add_message("System".to_string(), 
                                            format!("🤖 Automode: Checking if Claude needs permission for tools: {}", attempted_tools.join(", ")));
                                    }
                                    
                                    let instance_id_copy = instance.id;
                                    let session_id = instance.session_id.clone();
                                    let tx = self.message_tx.clone();
                                    let deepseek_tx = self.deepseek_tx.clone();
                                    let claude_msg_for_permission = claude_message.clone();
                                    
                                    tokio::spawn(async move {
                                        // Only check for permission issues if there were tool attempts
                                        if had_tool_attempts {
                                            tracing::info!("Claude attempted to use tools: {:?}, checking for permission issues", attempted_tools);
                                            
                                            // Check if Claude mentioned permission issues
                                            match check_tool_permission_issue(&claude_msg_for_permission, &attempted_tools).await {
                                                Ok(Some(tools)) => {
                                                    tracing::info!("Automode: Claude needs permission for tools: {:?}", tools);
                                                    
                                                    // Enable each tool that Claude needs
                                                    let mut enabled_tools = Vec::new();
                                                    for tool in &tools {
                                                        if let Err(e) = enable_claude_tool(tool).await {
                                                            tracing::error!("Failed to enable tool {}: {}", tool, e);
                                                        } else {
                                                            tracing::info!("Successfully enabled tool: {}", tool);
                                                            enabled_tools.push(tool.clone());
                                                        }
                                                    }
                                                    
                                                    if !enabled_tools.is_empty() {
                                                        // Send a system message to the UI
                                                        let system_msg = format!("🔧 Automode: Enabled tools: {}", enabled_tools.join(", "));
                                                        let _ = tx.send(ClaudeMessage::StreamText {
                                                            instance_id: instance_id_copy,
                                                            text: system_msg,
                                                        }).await;
                                                        
                                                        // Send a message telling Claude the tools are now enabled
                                                        let response = format!(
                                                            "I've enabled the following tools for you: {}. Please try using them again.",
                                                            enabled_tools.join(", ")
                                                        );
                                                        
                                                        if let Err(e) = send_to_claude_with_session(instance_id_copy, response, tx, session_id).await {
                                                            tracing::error!("Failed to send tool enablement message to Claude: {}", e);
                                                        }
                                                    }
                                                    return; // Don't process as regular question
                                                }
                                                Ok(None) => {
                                                    tracing::info!("No permission issues detected after tool attempts");
                                                }
                                                Err(e) => {
                                                    tracing::error!("Failed to check tool permissions: {}", e);
                                                }
                                            }
                                        } else {
                                            // No tool attempts, check if it's a regular question
                                            let (is_question, _) = analyze_claude_message(&claude_msg_for_permission);
                                            
                                            if is_question {
                                                tracing::info!("Automode: Claude asked a question, generating DeepSeek response");
                                                
                                                // Generate streaming response for UI display
                                                tokio::spawn(async move {
                                                    if let Err(e) = generate_deepseek_response_stream(
                                                        &claude_msg_for_permission, 
                                                        &user_context,
                                                        deepseek_tx
                                                    ).await {
                                                        tracing::error!("Failed to generate DeepSeek response: {}", e);
                                                    }
                                                });
                                            }
                                        }
                                    });
                                }
                            }
                        } else {
                            tracing::info!("Automode is OFF");
                        }
                    }
                }
                ClaudeMessage::Error { instance_id, error } => {
                    tracing::error!("Error for instance {}: {}", instance_id, error);
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        instance.add_message("Error".to_string(), error);
                        instance.is_processing = false;
                    }
                }
                ClaudeMessage::Exited { instance_id, code } => {
                    tracing::info!("Process exited for instance {} with code: {:?}", instance_id, code);
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        instance.is_processing = false;
                    }
                }
                ClaudeMessage::ToolUse { instance_id, tool_name } => {
                    tracing::info!("Tool use attempt for instance {}: {}", instance_id, tool_name);
                    
                    // Show todo list if TodoRead or TodoWrite is used
                    if tool_name == "TodoRead" || tool_name == "TodoWrite" {
                        self.show_todo_list();
                    }
                    
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        // Add tool use message to the conversation
                        instance.add_message("Tool".to_string(), format!("🔧 Attempting to use: {}", tool_name));
                        // Track this tool attempt
                        instance.last_tool_attempts.push(tool_name.clone());
                        
                        // Parse todo list from the next message if it's TodoRead/TodoWrite result
                        if tool_name == "TodoRead" || tool_name == "TodoWrite" {
                            // Mark that we're expecting todo data
                            instance.add_message("System".to_string(), "📋 Waiting for todo list data...".to_string());
                        }
                    }
                }
                ClaudeMessage::SessionStarted { instance_id, session_id } => {
                    tracing::info!("Session started for instance {} with ID: {}", instance_id, session_id);
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        instance.session_id = Some(session_id.clone());
                        instance.add_message("System".to_string(), format!("📝 Session started: {}", session_id));
                    }
                }
            }
        }
    }

    fn copy_selection(&mut self) -> Result<()> {
        let text_to_copy = if let Some(instance) = self.current_instance_mut() {
            instance.get_selected_text()
        } else {
            None
        };
        
        if let Some(text) = text_to_copy {
            if let Ok(mut clipboard) = self.clipboard.lock() {
                clipboard.set_text(text)?;
            }
            
            // Clear selection after copy
            if let Some(instance) = self.current_instance_mut() {
                instance.selection_start = None;
                instance.selection_end = None;
                instance.selecting = false;
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging to debug.log
    let file_appender = tracing_appender::rolling::never(".", "debug.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter("veda_tui=debug")
        .init();
    
    tracing::info!("Starting Veda TUI");
    
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new()?;

    // Run the UI
    let res = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        // Process any Claude messages
        app.process_claude_messages().await;
        
        // Process any DeepSeek messages
        app.process_deepseek_messages().await;
        
        // Check if todo list should be hidden
        if app.should_hide_todo_list() {
            app.hide_todo_list();
        }
        
        terminal.draw(|f| ui(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Paste(data) => {
                    // Handle paste event - insert text directly into textarea
                    if let Some(instance) = app.current_instance_mut() {
                        if !instance.is_processing {
                            tracing::debug!("Paste event detected with {} characters", data.len());
                            // Insert each character of the pasted data
                            for ch in data.chars() {
                                use ratatui::crossterm::event::{Event as RatatuiEvent, KeyEvent, KeyCode as RatatuiKeyCode, KeyModifiers as RatatuiKeyModifiers};
                                let key_event = if ch == '\n' {
                                    KeyEvent::new(RatatuiKeyCode::Enter, RatatuiKeyModifiers::NONE)
                                } else {
                                    KeyEvent::new(RatatuiKeyCode::Char(ch), RatatuiKeyModifiers::NONE)
                                };
                                instance.textarea.input(RatatuiEvent::Key(key_event));
                            }
                        }
                    }
                }
                Event::Key(key) => {
                    tracing::debug!("Key event: {:?} with modifiers: {:?}", key.code, key.modifiers);
                    match (key.modifiers, key.code) {
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                            // Check if we have a selection first
                            if let Some(instance) = app.current_instance_mut() {
                                if instance.selection_start.is_some() {
                                    app.copy_selection()?;
                                    continue;
                                }
                            }
                            // No selection, quit
                            return Ok(());
                        }
                        (_, KeyCode::Esc) => return Ok(()),
                        (KeyModifiers::CONTROL, KeyCode::Char('n')) => app.add_instance(),
                        (KeyModifiers::CONTROL, KeyCode::Char('a')) => app.toggle_auto_mode(),
                        (KeyModifiers::CONTROL, KeyCode::Char('t')) => app.toggle_chain_of_thought(),
                        (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                            if app.todo_list.visible {
                                app.hide_todo_list();
                            } else {
                                app.show_todo_list();
                            }
                        }
                        (KeyModifiers::CONTROL, KeyCode::Left) => app.previous_tab(),
                        (KeyModifiers::CONTROL, KeyCode::Right) => app.next_tab(),
                        (KeyModifiers::SHIFT, KeyCode::Enter) => {
                            // Shift+Enter adds a new line manually
                            if let Some(instance) = app.current_instance_mut() {
                                if !instance.is_processing {
                                    tracing::debug!("Shift+Enter pressed, adding new line");
                                    // Manually insert a new line
                                    instance.textarea.insert_newline();
                                }
                            }
                        }
                        (_, KeyCode::Enter) => {
                            // Enter sends message
                            if let Some(instance) = app.current_instance_mut() {
                                if !instance.textarea.is_empty() && !instance.is_processing {
                                    let message = instance.textarea.lines().join("\n");
                                    instance.textarea = TextArea::default();
                                    instance.textarea.set_block(
                                        Block::default()
                                            .borders(Borders::ALL)
                                            .title("Input")
                                    );
                                    app.send_message(message).await;
                                }
                            }
                        }
                        _ => {
                            // Pass all other key events to the textarea
                            if let Some(instance) = app.current_instance_mut() {
                                if !instance.is_processing {
                                    use ratatui::crossterm::event::Event as RatatuiEvent;
                                    instance.textarea.input(RatatuiEvent::Key(key));
                                }
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::Down(_) => {
                            if let Some(instance) = app.current_instance_mut() {
                                // Check if click is on a DeepSeek thinking message
                                let message_area_start = 3; // Account for header
                                
                                // Prevent underflow by checking bounds first
                                if mouse.row >= message_area_start {
                                    let clicked_line = (mouse.row - message_area_start) as usize;
                                    
                                    if clicked_line < instance.messages.len() {
                                        let msg = &mut instance.messages[clicked_line];
                                        if msg.sender == "DeepSeek" && msg.is_thinking {
                                            // Toggle collapsed state
                                            msg.is_collapsed = !msg.is_collapsed;
                                            continue;
                                        }
                                    }
                                }
                                
                                // Otherwise, start selection
                                instance.selecting = true;
                                instance.selection_start = Some((mouse.column, mouse.row));
                                instance.selection_end = Some((mouse.column, mouse.row));
                            }
                        }
                        MouseEventKind::Drag(_) => {
                            if let Some(instance) = app.current_instance_mut() {
                                if instance.selecting {
                                    instance.selection_end = Some((mouse.column, mouse.row));
                                }
                            }
                        }
                        MouseEventKind::Up(_) => {
                            if let Some(instance) = app.current_instance_mut() {
                                instance.selecting = false;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    // Calculate textarea height based on content
    let textarea_height = if let Some(instance) = app.instances.get(app.current_tab) {
        let line_count = instance.textarea.lines().len() as u16;
        // Minimum 3 (1 line + 2 borders), maximum 8 lines
        (line_count + 2).max(3).min(8)
    } else {
        3
    };
    
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(textarea_height),
        ])
        .split(f.area());

    // Header with tabs
    let titles: Vec<Line> = app
        .instances
        .iter()
        .map(|instance| Line::from(instance.name.clone()))
        .collect();
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Veda - Claude Orchestrator"))
        .select(app.current_tab)
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[0]);

    // Messages area
    if let Some(instance) = app.instances.get_mut(app.current_tab) {
        let mut all_lines = Vec::new();
        
        for (i, msg) in instance.messages.iter().enumerate() {
            let mut content = vec![
                Span::styled(&msg.timestamp, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    &msg.sender,
                    match msg.sender.as_str() {
                        "You" => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                        "Tool" => Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                        "System" => Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                        "Error" | "DeepSeek Error" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        "DeepSeek" => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        _ => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    },
                ),
                Span::raw(": "),
            ];
            
            // Handle DeepSeek thinking messages
            if msg.sender == "DeepSeek" && msg.is_thinking {
                if msg.is_collapsed || !app.show_chain_of_thought {
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
            } else {
                content.push(Span::raw(&msg.content));
            }
            
            // Apply selection highlighting
            let mut style = Style::default();
            if let (Some(start), Some(end)) = (instance.selection_start, instance.selection_end) {
                let line_y = i as u16;
                let start_y = start.1.min(end.1);
                let end_y = start.1.max(end.1);
                
                if line_y >= start_y && line_y <= end_y {
                    style = style.add_modifier(Modifier::REVERSED);
                }
            }
            
            all_lines.push(Line::from(content).style(style));
            // Add empty line between messages for readability
            all_lines.push(Line::from(""));
        }

        let messages_paragraph = Paragraph::new(all_lines)
            .block(Block::default().borders(Borders::ALL).title(format!(
                "Messages - {} [Auto: {}] [CoT: {}]{}",
                instance.name,
                if app.auto_mode { "ON" } else { "OFF" },
                if app.show_chain_of_thought { "ON" } else { "OFF" },
                if let Some(ref sid) = instance.session_id {
                    let display_len = 8.min(sid.len());
                    let start = sid.len().saturating_sub(display_len);
                    format!(" [Session: ...{}]", &sid[start..])
                } else {
                    String::new()
                }
            )))
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: true })
            .scroll((instance.scroll_offset, 0));
        f.render_widget(messages_paragraph, chunks[1]);

        // Input area with tui-textarea
        let title = if instance.is_processing {
            "Input (Processing...)"
        } else {
            "Input (Enter to send, Shift+Enter for new line)"
        };
        
        instance.textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
        );
        f.render_widget(&instance.textarea, chunks[2]);
    }
    
    // Render todo list overlay if visible
    if app.todo_list.visible {
        render_todo_overlay(f, &app.todo_list);
    }
}

fn render_todo_overlay(f: &mut Frame, todo_list: &TodoListState) {
    let area = f.area();
    
    // Calculate overlay size based on content
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("📋 ", Style::default()),
        Span::styled("Todo List", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from("")); // Empty line
    
    if todo_list.items.is_empty() {
        lines.push(Line::from(Span::styled(
            "No tasks yet", 
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
        )));
    } else {
        for item in &todo_list.items {
            let status_emoji = match item.status.as_str() {
                "done" => "✅",
                "in-progress" => "🔄",
                "review" => "👀",
                "deferred" => "⏸️",
                "cancelled" => "❌",
                _ => "⬜",
            };
            
            let priority_color = match item.priority.as_str() {
                "high" => Color::Red,
                "low" => Color::Blue,
                _ => Color::Yellow,
            };
            
            lines.push(Line::from(vec![
                Span::raw(format!("{} ", status_emoji)),
                Span::styled(&item.id, Style::default().fg(Color::DarkGray)),
                Span::raw(": "),
                Span::styled(&item.content, Style::default().fg(priority_color)),
            ]));
        }
    }
    
    // Calculate dimensions
    let max_width = lines.iter()
        .map(|l| l.width())
        .max()
        .unwrap_or(20)
        .min(area.width as usize - 4) as u16 + 4;
    let height = (lines.len() as u16 + 2).min(area.height - 4);
    
    // Center the overlay
    let x = (area.width.saturating_sub(max_width)) / 2;
    let y = 2; // Near the top
    
    let popup_area = Rect {
        x,
        y,
        width: max_width,
        height,
    };
    
    // Clear the area first
    f.render_widget(Clear, popup_area);
    
    // Create the todo list widget
    let todo_widget = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .style(Style::default().bg(Color::Black)))
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left);
    
    f.render_widget(todo_widget, popup_area);
}

