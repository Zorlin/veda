mod claude;
mod deepseek;
mod shared_ipc;

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
                      generate_deepseek_stall_response, check_tool_permission_issue, DeepSeekMessage};

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
    // Track successful tool usage to avoid unnecessary permission checks
    successful_tools: Vec<String>,
    // Claude session ID for resume
    session_id: Option<String>,
    // Working directory for this tab
    working_directory: String,
    // Stall detection
    last_activity: DateTime<Local>,
    stall_check_sent: bool,
    stall_delay_seconds: i64, // Dynamic delay: 10s, 20s, 30s max
    stall_intervention_in_progress: bool, // Prevent multiple simultaneous interventions
    // Store last known terminal dimensions for auto-scrolling
    last_terminal_width: u16,
    last_message_area_height: u16,
    // Process handle for interruption
    process_handle: Option<Arc<tokio::sync::Mutex<Option<tokio::process::Child>>>>,
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
            successful_tools: Vec::new(),
            session_id: None,
            working_directory: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            last_activity: Local::now(),
            stall_check_sent: false,
            stall_delay_seconds: 10, // Start with 10 second delay
            stall_intervention_in_progress: false,
            last_terminal_width: 80, // Default terminal width
            last_message_area_height: 20, // Default message area height
            process_handle: None,
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
        // Update activity when new messages arrive
        self.last_activity = Local::now();
        // Reset stall check flags when there's new activity
        if !is_thinking {
            self.stall_check_sent = false;
            self.stall_intervention_in_progress = false;
        }
        
        // Auto-scroll to show new messages using last known dimensions
        self.auto_scroll_with_width(Some(self.last_message_area_height), Some(self.last_terminal_width));
    }
    
    fn auto_scroll_to_bottom(&mut self, message_area_height: Option<u16>) {
        self.auto_scroll_with_width(message_area_height, None);
    }
    
    fn auto_scroll_with_width(&mut self, message_area_height: Option<u16>, terminal_width: Option<u16>) {
        // Count actual rendered lines
        let mut total_lines = 0;
        let term_width = terminal_width.unwrap_or(80).saturating_sub(2); // Account for borders
        
        for msg in &self.messages {
            if msg.sender == "DeepSeek" && msg.is_thinking && (msg.is_collapsed || !self.messages.is_empty()) {
                // Collapsed thinking shows as one line
                total_lines += 1;
            } else {
                // Build the full line as it will be rendered
                let prefix = format!("{} {}: ", msg.timestamp, msg.sender);
                let prefix_len = unicode_width::UnicodeWidthStr::width(prefix.as_str());
                
                // For first line of content
                let content_lines: Vec<&str> = msg.content.lines().collect();
                if content_lines.is_empty() || (content_lines.len() == 1 && content_lines[0].is_empty()) {
                    // Empty message still takes one line
                    total_lines += 1;
                } else {
                    // Calculate wrapped lines for the actual content
                    for (i, line) in content_lines.iter().enumerate() {
                        if i == 0 {
                            // First line includes the prefix
                            let first_line_width = prefix_len + unicode_width::UnicodeWidthStr::width(*line);
                            let wrapped = (first_line_width as f32 / term_width as f32).ceil() as usize;
                            total_lines += wrapped.max(1);
                        } else {
                            // Subsequent lines don't have prefix
                            let line_width = unicode_width::UnicodeWidthStr::width(*line);
                            if line_width == 0 {
                                total_lines += 1; // Empty line
                            } else {
                                let wrapped = (line_width as f32 / term_width as f32).ceil() as usize;
                                total_lines += wrapped.max(1);
                            }
                        }
                    }
                }
            }
            
            // Add empty line between messages
            total_lines += 1;
        }
        
        // Use provided height or fallback to default  
        let visible_lines = message_area_height.unwrap_or(20) as usize;
        
        // Account for borders (2 lines) and ensure we don't over-scroll
        let actual_visible = visible_lines.saturating_sub(2);
        
        if total_lines > actual_visible {
            // Calculate scroll to show bottom content
            // Subtract 1 to ensure the last line is visible
            self.scroll_offset = (total_lines.saturating_sub(actual_visible)) as u16;
            // Ensure scroll offset doesn't exceed reasonable bounds
            if self.scroll_offset > 10000 {
                tracing::error!("Excessive scroll offset detected: {}, resetting to safe value", self.scroll_offset);
                self.scroll_offset = (total_lines.saturating_sub(actual_visible)).min(1000) as u16;
            }
        } else {
            self.scroll_offset = 0;
        }
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
    
    fn should_check_for_stall(&self) -> bool {
        if self.is_processing || self.stall_check_sent || self.stall_intervention_in_progress {
            return false;
        }
        
        // Don't trigger stall detection if user hasn't sent their first message
        let has_user_message = self.messages.iter().any(|m| m.sender == "You");
        if !has_user_message {
            return false;
        }
        
        let elapsed = Local::now().signed_duration_since(self.last_activity);
        elapsed.num_seconds() > self.stall_delay_seconds
    }
    
    fn on_user_input(&mut self) {
        // Update activity and increase delay when user types
        self.last_activity = Local::now();
        self.stall_check_sent = false;
        
        // Double the delay up to 30 seconds max when user types
        self.stall_delay_seconds = (self.stall_delay_seconds * 2).min(30);
        tracing::debug!("User input detected, stall delay increased to {} seconds", self.stall_delay_seconds);
    }
    
    fn get_recent_context(&self) -> (String, String) {
        // Get the last Claude message
        let claude_message = self.messages.iter()
            .rev()
            .find(|m| m.sender == "Claude" && !m.content.is_empty())
            .map(|m| m.content.clone())
            .unwrap_or_default();
            
        // Get the last few user messages for context
        let user_context = self.messages.iter()
            .rev()
            .filter(|m| m.sender == "You")
            .take(3)
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n\n");
            
        (claude_message, user_context)
    }
}

struct App {
    // Global process ID (PID) for this Veda process
    instance_id: u32,
    // Session ID for this Veda process
    session_id: String,
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
    // Terminal size and tab rectangles
    terminal_width: u16,
    tab_rects: Vec<Rect>,
    // Multi-instance coordination
    coordination_enabled: bool,
    max_instances: usize,
    coordination_in_progress: bool,
    // Rate limiting for coordination skip log
    last_coordination_skip_log: Option<std::time::Instant>,
    // Message queue system (like Claude Code)
    message_queue: Vec<String>,
    // Triple-Enter interruption detection
    enter_press_count: u8,
    last_enter_time: Option<std::time::Instant>,
    // Buffer for messages that arrive before sessions are established
    pending_session_messages: Vec<(u32, String, String)>, // (process_id, text, session_id)
    // No complex mapping needed - shared registry handles cross-process coordination
    // Auto-task to send once main instance has session ID
    pending_auto_task: Option<String>,
}

impl App {
    fn strip_chain_of_thought(text: &str) -> String {
        let mut cleaned = text.to_string();
        
        // Remove <thinking>...</thinking> blocks
        while let Some(start) = cleaned.find("<thinking>") {
            if let Some(end) = cleaned[start..].find("</thinking>") {
                let end_pos = start + end + "</thinking>".len();
                cleaned.replace_range(start..end_pos, "");
            } else {
                break;
            }
        }
        
        // Split by lines and filter out obvious thinking patterns
        let lines: Vec<&str> = cleaned.lines().collect();
        let mut filtered_lines = Vec::new();
        let mut skip_until_empty = false;
        
        for line in lines {
            let line_lower = line.to_lowercase();
            
            // Skip lines that start thinking patterns
            if line_lower.starts_with("let me think") ||
               line_lower.starts_with("i need to") ||
               line_lower.starts_with("first, i") ||
               line_lower.starts_with("analysis:") ||
               line_lower.contains("let me analyze") {
                skip_until_empty = true;
                continue;
            }
            
            // Reset skip flag on empty line or clear content
            if line.trim().is_empty() {
                skip_until_empty = false;
                filtered_lines.push(line);
                continue;
            }
            
            // Skip if we're in a thinking section
            if skip_until_empty {
                continue;
            }
            
            filtered_lines.push(line);
        }
        
        let result = filtered_lines.join("\n");
        
        // Clean up extra whitespace
        let mut final_result = result.trim().to_string();
        // Remove multiple consecutive newlines
        while final_result.contains("\n\n\n") {
            final_result = final_result.replace("\n\n\n", "\n\n");
        }
        
        final_result
    }
    
    fn is_tool_whitelisted(tool_name: &str) -> bool {
        // Standard Claude Code utilities and known safe tools
        let safe_tools = [
            // Basic file operations
            "Read", "Write", "Edit", "MultiEdit", "Glob", "Grep", "LS",
            // Command execution
            "Bash",
            // Todo management
            "TodoRead", "TodoWrite",
            // Notebook operations
            "NotebookRead", "NotebookEdit",
            // Web fetching (read-only)
            "WebFetch", "WebSearch",
            // TaskMaster AI tools
            "mcp__taskmaster-ai__initialize_project",
            "mcp__taskmaster-ai__models",
            "mcp__taskmaster-ai__parse_prd",
            "mcp__taskmaster-ai__get_tasks",
            "mcp__taskmaster-ai__get_task",
            "mcp__taskmaster-ai__next_task",
            "mcp__taskmaster-ai__complexity_report",
            "mcp__taskmaster-ai__set_task_status",
            "mcp__taskmaster-ai__generate",
            "mcp__taskmaster-ai__add_task",
            "mcp__taskmaster-ai__add_subtask",
            "mcp__taskmaster-ai__update",
            "mcp__taskmaster-ai__update_task",
            "mcp__taskmaster-ai__update_subtask",
            "mcp__taskmaster-ai__remove_task",
            "mcp__taskmaster-ai__remove_subtask",
            "mcp__taskmaster-ai__clear_subtasks",
            "mcp__taskmaster-ai__move_task",
            "mcp__taskmaster-ai__analyze_project_complexity",
            "mcp__taskmaster-ai__expand_task",
            "mcp__taskmaster-ai__expand_all",
            "mcp__taskmaster-ai__add_dependency",
            "mcp__taskmaster-ai__remove_dependency",
            "mcp__taskmaster-ai__validate_dependencies",
            "mcp__taskmaster-ai__fix_dependencies",
            // DeepWiki tools
            "mcp__deepwiki__read_wiki_structure",
            "mcp__deepwiki__read_wiki_contents",
            "mcp__deepwiki__ask_question",
            // Playwright tools (for testing)
            "mcp__playwright__browser_close",
            "mcp__playwright__browser_resize",
            "mcp__playwright__browser_console_messages",
            "mcp__playwright__browser_handle_dialog",
            "mcp__playwright__browser_file_upload",
            "mcp__playwright__browser_install",
            "mcp__playwright__browser_press_key",
            "mcp__playwright__browser_navigate",
            "mcp__playwright__browser_navigate_back",
            "mcp__playwright__browser_navigate_forward",
            "mcp__playwright__browser_network_requests",
            "mcp__playwright__browser_pdf_save",
            "mcp__playwright__browser_take_screenshot",
            "mcp__playwright__browser_snapshot",
            "mcp__playwright__browser_click",
            "mcp__playwright__browser_drag",
            "mcp__playwright__browser_hover",
            "mcp__playwright__browser_type",
            "mcp__playwright__browser_select_option",
            "mcp__playwright__browser_tab_list",
            "mcp__playwright__browser_tab_new",
            "mcp__playwright__browser_tab_select",
            "mcp__playwright__browser_tab_close",
            "mcp__playwright__browser_generate_playwright_test",
            "mcp__playwright__browser_wait_for",
            // Veda instance management tools (always allowed)
            "mcp__veda__veda_spawn_instances",
            "mcp__veda__veda_list_instances",
            "mcp__veda__veda_close_instance",
        ];
        
        safe_tools.contains(&tool_name)
    }
    
    async fn analyze_tool_safety(tool_name: &str) -> Result<bool> {
        // Check whitelist first - skip expensive analysis for known safe tools
        if Self::is_tool_whitelisted(tool_name) {
            tracing::info!("Tool {} is whitelisted as safe, auto-approving", tool_name);
            return Ok(true);
        }
        
        tracing::info!("Analyzing safety of non-whitelisted tool: {}", tool_name);
        
        let prompt = format!(
            r#"Analyze if it's safe to automatically enable the "{}" tool for Claude.

Consider these security factors:
1. Can this tool be used maliciously to harm the system?
2. Can it access or modify sensitive data?
3. Can it execute arbitrary commands that could be dangerous?
4. Is this a commonly safe tool for AI assistants?

Common safe tools: Read, Write (for basic file operations), TodoRead, TodoWrite, mcp tools
Potentially unsafe tools: Bash (arbitrary command execution), tools that access network/system

Your response must be EXACTLY one of:
SAFE_TO_ENABLE
UNSAFE_TO_ENABLE

Your response:"#,
            tool_name
        );
        
        let request_body = serde_json::json!({
            "model": "gemma3:12b",
            "prompt": prompt,
            "stream": false
        });
        
        let client = reqwest::Client::new();
        let response = client
            .post("http://localhost:11434/api/generate")
            .json(&request_body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            tracing::error!("Ollama API error: {}", error_text);
            return Err(anyhow::anyhow!("Ollama API error: {}", error_text));
        }
        
        #[derive(serde::Deserialize)]
        struct OllamaResponse {
            response: String,
        }
        
        let ollama_response: OllamaResponse = response.json().await?;
        let response_text = ollama_response.response.trim();
        
        tracing::debug!("DeepSeek safety analysis response: {}", response_text);
        
        // Extract the final verdict, ignoring chain of thought
        // Look for the LAST occurrence to get the final verdict
        let verdict = if response_text.rfind("UNSAFE_TO_ENABLE").unwrap_or(0) > 
                         response_text.rfind("SAFE_TO_ENABLE").unwrap_or(0) {
            "UNSAFE_TO_ENABLE"
        } else if response_text.contains("SAFE_TO_ENABLE") {
            "SAFE_TO_ENABLE"
        } else {
            // Default to unsafe if unclear
            "UNSAFE_TO_ENABLE"
        };
        
        let is_safe = verdict == "SAFE_TO_ENABLE";
        tracing::info!("Tool {} safety analysis: {} -> {}", tool_name, verdict, if is_safe { "APPROVED" } else { "DENIED" });
        Ok(is_safe)
    }
    
    fn new() -> Result<Self> {
        let mut instances = Vec::new();
        instances.push(ClaudeInstance::new("Veda-1".to_string()));
        
        // No complex initialization needed
        
        let (tx, rx) = mpsc::channel(100);
        let (deepseek_tx, deepseek_rx) = mpsc::channel(100);
        
        // Use the actual PID as the Veda process ID
        let instance_id = std::process::id();
        let session_id = Uuid::new_v4().to_string();
        tracing::info!("Veda process started with PID: {}, session ID: {}", instance_id, session_id);
        
        Ok(Self {
            instance_id,
            session_id,
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
            terminal_width: 80, // Default, will be updated in draw
            tab_rects: Vec::new(),
            coordination_enabled: true,
            max_instances: 5, // Main + 4 additional
            coordination_in_progress: false,
            last_coordination_skip_log: None,
            message_queue: Vec::new(),
            enter_press_count: 0,
            last_enter_time: None,
            pending_session_messages: Vec::new(),
            pending_auto_task: None,
        })
    }

    fn current_instance(&self) -> Option<&ClaudeInstance> {
        self.instances.get(self.current_tab)
    }
    
    fn current_instance_mut(&mut self) -> Option<&mut ClaudeInstance> {
        self.instances.get_mut(self.current_tab)
    }
    
    fn assign_session_to_instance(&mut self, target_instance_index: Option<usize>, session_id: String) {
        let tab_info = target_instance_index
            .map(|idx| format!("Tab {} ({})", idx + 1, self.instances[idx].name.clone()))
            .unwrap_or_else(|| "Unknown tab".to_string());
        
        tracing::info!("🎬 Session started for {} with ID: {}", tab_info, session_id);
        
        // Log all current instances for debugging
        for (i, inst) in self.instances.iter().enumerate() {
            tracing::info!("  Instance {}: {} (ID: {}, Session: {:?})", 
                i + 1, inst.name, inst.id, inst.session_id);
        }
        
        if let Some(instance_idx) = target_instance_index {
            let instance = &mut self.instances[instance_idx];
            instance.session_id = Some(session_id.clone());
            instance.add_message("System".to_string(), format!("📝 Session started: {}", session_id));
            tracing::info!("✅ Successfully set session {} for {}", session_id, instance.name);
            
            // If this is instance 0 and we have a pending auto-task, send it
            if instance_idx == 0 && self.pending_auto_task.is_some() {
                let auto_task = self.pending_auto_task.take().unwrap();
                let tx = self.message_tx.clone();
                let session_id_for_auto = session_id.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    let _ = tx.send(ClaudeMessage::StreamText {
                        text: auto_task,
                        session_id: Some(session_id_for_auto),
                    }).await;
                });
                tracing::info!("🚀 Sent pending auto-task to instance 0 with session {}", session_id);
            }
            
            // Process any buffered messages for this session
            let mut buffered_messages = Vec::new();
            let mut remaining_messages = Vec::new();
            
            for (msg_instance_id, text, msg_session_id) in std::mem::take(&mut self.pending_session_messages) {
                if msg_session_id == session_id {
                    buffered_messages.push((msg_instance_id, text, msg_session_id));
                } else {
                    remaining_messages.push((msg_instance_id, text, msg_session_id));
                }
            }
            self.pending_session_messages = remaining_messages;
            
            if !buffered_messages.is_empty() {
                tracing::info!("📬 Processing {} buffered messages for session {}", buffered_messages.len(), session_id);
                for (_, text, _) in buffered_messages {
                    instance.add_message("System".to_string(), text);
                }
            }
        }
    }

    fn add_instance(&mut self) {
        let instance_num = self.instances.len() + 1;
        let instance_name = format!("Veda-{}", instance_num);
        let new_instance = ClaudeInstance::new(instance_name.clone());
        
        tracing::info!("Creating new tab: {}", instance_name);
        
        self.instances.push(new_instance);
        self.current_tab = self.instances.len() - 1;
        
        // Tab created - session ID will be assigned when user first sends a message
        tracing::info!("✅ New tab {} created (session ID will be assigned on first use)", instance_name);
    }
    
    fn close_current_instance(&mut self) {
        if self.instances.len() > 1 {
            self.instances.remove(self.current_tab);
            // Adjust current tab if we removed the last one
            if self.current_tab >= self.instances.len() {
                self.current_tab = self.instances.len() - 1;
            }
            self.sync_working_directory();
        }
        // If only one tab left, don't close it (always keep at least one)
    }
    
    fn sync_working_directory(&mut self) {
        if let Some(instance) = self.instances.get(self.current_tab) {
            if let Err(e) = std::env::set_current_dir(&instance.working_directory) {
                tracing::warn!("Failed to sync working directory to {}: {}", instance.working_directory, e);
            } else {
                tracing::debug!("Synced working directory to: {}", instance.working_directory);
            }
        }
    }

    fn next_tab(&mut self) {
        if !self.instances.is_empty() {
            self.current_tab = (self.current_tab + 1) % self.instances.len();
            self.sync_working_directory();
        }
    }

    fn previous_tab(&mut self) {
        if !self.instances.is_empty() {
            self.current_tab = if self.current_tab == 0 {
                self.instances.len() - 1
            } else {
                self.current_tab - 1
            };
            self.sync_working_directory();
        }
    }

    fn toggle_auto_mode(&mut self) {
        self.auto_mode = !self.auto_mode;
    }

    fn toggle_chain_of_thought(&mut self) {
        self.show_chain_of_thought = !self.show_chain_of_thought;
    }
    
    fn toggle_coordination_mode(&mut self) {
        self.coordination_enabled = !self.coordination_enabled;
        let status = if self.coordination_enabled { "ENABLED" } else { "DISABLED" };
        if let Some(instance) = self.current_instance_mut() {
            instance.add_message("System".to_string(), 
                format!("🤝 Multi-instance coordination {}", status));
        }
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

    fn create_capabilities_prompt() -> String {
        r#"🔧 **VEDA CLAUDE CAPABILITIES OVERVIEW**

You are running in Veda, a powerful Claude Code environment with enhanced coordination capabilities.

**🛠️ AVAILABLE MCP TOOLS:**
• **TaskMaster AI**: Complete project management - use for planning, tracking, and coordinating tasks
  - `mcp__taskmaster-ai__*` tools for task management, PRD parsing, and project coordination
• **Playwright**: Browser automation and testing
  - `mcp__playwright__*` tools for web interaction, testing, and automation
• **DeepWiki**: Repository analysis and documentation
  - `mcp__deepwiki__*` tools for understanding codebases and documentation

**🤝 MULTI-INSTANCE COORDINATION:**
You can spawn additional Claude instances for parallel processing:
• **`veda_spawn_instances`**: Create 1-3 additional Claude instances for complex tasks
  - Use for: large codebases, multiple features, parallel development streams
  - Each instance gets assigned specific scopes/directories to avoid conflicts
• **`veda_list_instances`**: View all active Claude instances and their status  
• **`veda_close_instance`**: Close specific instances when tasks are complete

**💡 COORDINATION STRATEGY:**
- For complex multi-part tasks, consider spawning additional instances
- Use TaskMaster tools to coordinate between instances and track progress
- Each instance should focus on specific files/modules to avoid conflicts
- Update main instance (Tab 1) with progress and use TaskMaster for synchronization

**🎯 BEST PRACTICES:**
- Always check available tools with the right MCP prefixes
- Use coordination for tasks that can be parallelized effectively
- Leverage TaskMaster for project organization and progress tracking
- Use Playwright for any web-related testing or automation needs

This prompt appears only once per session. You now have full access to these powerful capabilities!"#.to_string()
    }

    async fn send_message(&mut self, message: String) {
        // Handle !cd command
        if message.trim().starts_with("!cd ") {
            let path = message.trim().strip_prefix("!cd ").unwrap_or("").trim();
            self.handle_cd_command(path).await;
            return;
        }
        
        // Handle !max command
        if message.trim().starts_with("!max ") {
            let max_str = message.trim().strip_prefix("!max ").unwrap_or("").trim();
            self.handle_max_command(max_str).await;
            return;
        }
        
        // Collect necessary data first to avoid borrowing conflicts
        let current_tab = self.current_tab;
        let (session_id, working_dir, is_first_message, process_handle, instance_name) = {
            if let Some(instance) = self.current_instance_mut() {
                // Log the current state for debugging
                let instance_name = instance.name.clone();
                tracing::info!("Sending message from tab {} ({}), session_id: {:?}", 
                    current_tab, instance_name, instance.session_id);
                tracing::info!("Message: {}", message);
                
                // Check if this is the first message BEFORE adding it
                let is_first_message = instance.messages.is_empty();
                instance.add_message("You".to_string(), message.clone());
                instance.is_processing = true;
                
                // Only use session_id for routing - eliminate instance_id from message flow
                let session_id = instance.session_id.clone();
                let working_dir = instance.working_directory.clone();
                
                // Create a new process handle storage if this is the first message
                let process_handle = if instance.process_handle.is_none() {
                    let handle = Arc::new(tokio::sync::Mutex::new(None));
                    instance.process_handle = Some(handle.clone());
                    Some(handle)
                } else {
                    instance.process_handle.clone()
                };
                
                (session_id, working_dir, is_first_message, process_handle, instance_name)
            } else {
                return;
            }
        };
        
        let tx = self.message_tx.clone();
        
        // Create the message to send
        let mut context_message = format!("Working directory: {}\n\n", working_dir);
        
        // Add capabilities prompt for first message in a session
        if is_first_message {
            tracing::info!("Adding capabilities prompt for first message in session");
            context_message.push_str(&Self::create_capabilities_prompt());
            context_message.push_str("\n\n---\n\n");
        } else {
            tracing::debug!("Not the first message, skipping capabilities prompt");
        }
        
        context_message.push_str(&message);
        tracing::debug!("Final message to Claude (first 200 chars): {}", &context_message.chars().take(200).collect::<String>());
        
        // Log which tab is sending the message
        tracing::info!("Tab {} ({}) sending message to Claude session {:?}", 
            current_tab + 1, 
            instance_name,
            session_id);
        
        // Send to Claude (no instance_id needed - only session_id for routing)
        tokio::spawn(async move {
            tracing::info!("Spawning send_to_claude task with session {:?} in dir {}", session_id, working_dir);
            if let Err(e) = send_to_claude_with_session(context_message, tx, session_id, process_handle).await {
                tracing::error!("Error sending to Claude: {}", e);
                eprintln!("Error sending to Claude: {}", e);
            } else {
                tracing::info!("Successfully initiated Claude command for message: {}", message);
            }
        });
    }
    
    async fn handle_cd_command(&mut self, path: &str) {
        if let Some(instance) = self.current_instance_mut() {
            let expanded_path = if path.starts_with('~') {
                path.replacen('~', &std::env::var("HOME").unwrap_or_default(), 1)
            } else {
                path.to_string()
            };
            
            // Add user message showing the command
            instance.add_message("You".to_string(), format!("!cd {}", path));
            
            // Validate the path exists and change to it
            match std::fs::metadata(&expanded_path) {
                Ok(metadata) if metadata.is_dir() => {
                    // Actually change the working directory globally
                    match std::env::set_current_dir(&expanded_path) {
                        Ok(_) => {
                            // Update the working directory for this tab
                            instance.working_directory = expanded_path.clone();
                            
                            instance.add_message(
                                "System".to_string(), 
                                format!("📁 Changed working directory to: {}", expanded_path)
                            );
                            tracing::info!("Changed working directory to: {} for tab {}", expanded_path, instance.name);
                        }
                        Err(e) => {
                            instance.add_message(
                                "System".to_string(), 
                                format!("❌ Failed to change to directory: {}", e)
                            );
                            tracing::error!("Failed to change to directory {}: {}", expanded_path, e);
                        }
                    }
                }
                Ok(_) => {
                    instance.add_message(
                        "System".to_string(), 
                        format!("❌ Path exists but is not a directory: {}", expanded_path)
                    );
                }
                Err(e) => {
                    instance.add_message(
                        "System".to_string(), 
                        format!("❌ Directory does not exist: {}", e)
                    );
                    tracing::error!("Failed to access directory {}: {}", expanded_path, e);
                }
            }
        }
    }

    async fn handle_max_command(&mut self, max_str: &str) {
        // Add user message showing the command first
        if let Some(instance) = self.current_instance_mut() {
            instance.add_message("You".to_string(), format!("!max {}", max_str));
        }
        
        // Parse the max instances value
        match max_str.trim().parse::<usize>() {
            Ok(new_max) if new_max > 0 && new_max <= 20 => {
                let old_max = self.max_instances;
                self.max_instances = new_max;
                
                if let Some(instance) = self.current_instance_mut() {
                    instance.add_message(
                        "System".to_string(), 
                        format!("⚙️ Max instances changed from {} to {}", old_max, new_max)
                    );
                }
                tracing::info!("Max instances changed from {} to {} for session", old_max, new_max);
                
                // If we now exceed the limit, schedule excess instances for shutdown
                if self.instances.len() > new_max {
                    let excess_count = self.instances.len() - new_max;
                    if let Some(instance) = self.current_instance_mut() {
                        instance.add_message(
                            "System".to_string(), 
                            format!("🔄 {} instances exceed the new limit and will shut down after completing current tasks", excess_count)
                        );
                    }
                    
                    // Mark excess instances for shutdown (starting from the end, keeping main instance)
                    for i in (new_max..self.instances.len()).rev() {
                        if i > 0 { // Never shut down the main instance (index 0)
                            if let Some(instance_to_shutdown) = self.instances.get_mut(i) {
                                instance_to_shutdown.add_message(
                                    "System".to_string(), 
                                    "🚪 This instance will shut down after completing current task due to new max limit".to_string()
                                );
                            }
                        }
                    }
                    
                    // Trigger graceful shutdown process
                    self.shutdown_excess_instances().await;
                } else {
                    let instances_len = self.instances.len();
                    if let Some(instance) = self.current_instance_mut() {
                        instance.add_message(
                            "System".to_string(), 
                            format!("✅ Current instance count ({}) is within the new limit", instances_len)
                        );
                    }
                }
            }
            Ok(new_max) if new_max > 20 => {
                if let Some(instance) = self.current_instance_mut() {
                    instance.add_message(
                        "System".to_string(), 
                        "❌ Maximum instance limit cannot exceed 20".to_string()
                    );
                }
            }
            Ok(_) => {
                if let Some(instance) = self.current_instance_mut() {
                    instance.add_message(
                        "System".to_string(), 
                        "❌ Maximum instance limit must be at least 1".to_string()
                    );
                }
            }
            Err(_) => {
                if let Some(instance) = self.current_instance_mut() {
                    instance.add_message(
                        "System".to_string(), 
                        format!("❌ Invalid number format: '{}'. Usage: !max <number>", max_str)
                    );
                }
            }
        }
    }

    async fn shutdown_excess_instances(&mut self) {
        // Identify instances that should be shut down (beyond max_instances limit)
        if self.instances.len() <= self.max_instances {
            return; // No excess instances
        }
        
        let instances_to_remove = self.instances.len() - self.max_instances;
        tracing::info!("Shutting down {} excess instances", instances_to_remove);
        
        // Remove excess instances from the end (keep main instance at index 0)
        let mut removed_count = 0;
        while self.instances.len() > self.max_instances && removed_count < instances_to_remove {
            let last_index = self.instances.len() - 1;
            if last_index > 0 { // Never remove the main instance
                let removed_instance = self.instances.remove(last_index);
                tracing::info!("Shut down instance: {} (ID: {})", removed_instance.name, removed_instance.id);
                
                // If we were on the removed tab, switch to the previous tab
                if self.current_tab >= self.instances.len() {
                    self.current_tab = self.instances.len().saturating_sub(1);
                }
                
                removed_count += 1;
            } else {
                break; // Don't remove the main instance
            }
        }
        
        // Log the new state
        let instances_len = self.instances.len();
        let max_instances = self.max_instances;
        if let Some(instance) = self.current_instance_mut() {
            instance.add_message(
                "System".to_string(), 
                format!("✅ Successfully shut down {} excess instances. Current count: {}/{}", 
                       removed_count, instances_len, max_instances)
            );
        }
    }

    async fn interrupt_current_instance(&mut self) {
        // First, extract the process handle and instance ID to avoid borrowing conflicts
        let (process_handle, instance_id) = {
            if let Some(instance) = self.current_instance_mut() {
                instance.add_message("System".to_string(), 
                    "⛔ Interrupting current process (triple-Enter detected)...".to_string());
                
                let process_handle = instance.process_handle.clone();
                let instance_id = instance.id;
                instance.is_processing = false;
                (process_handle, instance_id)
            } else {
                return;
            }
        };
        
        // Send SIGINT to the actual Claude process if we have a handle
        if let Some(process_handle) = process_handle {
            let mut handle_guard = process_handle.lock().await;
            if let Some(ref mut child) = *handle_guard {
                #[cfg(unix)]
                {
                    use nix::sys::signal::{self, Signal};
                    use nix::unistd::Pid;
                    
                    if let Some(pid) = child.id() {
                        tracing::info!("Sending SIGINT to Claude process with PID {}", pid);
                        match signal::kill(Pid::from_raw(pid as i32), Signal::SIGINT) {
                            Ok(_) => {
                                if let Some(instance) = self.current_instance_mut() {
                                    instance.add_message("System".to_string(), 
                                        "📡 SIGINT sent to Claude process".to_string());
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to send SIGINT to process {}: {}", pid, e);
                                if let Some(instance) = self.current_instance_mut() {
                                    instance.add_message("System".to_string(), 
                                        format!("⚠️ Failed to send interrupt signal: {}", e));
                                }
                            }
                        }
                    }
                }
                
                #[cfg(not(unix))]
                {
                    tracing::warn!("SIGINT not supported on this platform, process will continue");
                    if let Some(instance) = self.current_instance_mut() {
                        instance.add_message("System".to_string(), 
                            "⚠️ Process interruption not supported on this platform".to_string());
                    }
                }
            }
        } else {
            tracing::warn!("No process handle available for interruption");
            if let Some(instance) = self.current_instance_mut() {
                instance.add_message("System".to_string(), 
                    "⚠️ No active process to interrupt".to_string());
            }
        }
        
        tracing::info!("Interrupted instance {}, will process queued messages", instance_id);
        
        // Immediately process the queue
        self.process_message_queue().await;
    }
    
    async fn process_message_queue(&mut self) {
        // Check if we should process queue and extract queue data
        let (should_process, queue_count) = {
            if let Some(instance) = self.current_instance() {
                (!instance.is_processing && !self.message_queue.is_empty(), self.message_queue.len())
            } else {
                (false, 0)
            }
        };
        
        if should_process {
            // Combine all queued messages
            let combined_message = if queue_count == 1 {
                self.message_queue.remove(0)
            } else {
                let combined = self.message_queue.join("\n\n");
                self.message_queue.clear();
                format!("Multiple messages:\n\n{}", combined)
            };
            
            // Add system message
            if let Some(instance) = self.current_instance_mut() {
                instance.add_message("System".to_string(), 
                    format!("📤 Processing {} queued message(s)", 
                        if combined_message.contains("Multiple messages:") { 
                            queue_count
                        } else { 
                            1 
                        }
                    ));
            }
            
            // Send the combined message
            self.send_message(combined_message).await;
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
                        let should_scroll = if let Some(last_msg) = instance.messages.iter_mut()
                            .rev()
                            .find(|m| m.sender == "DeepSeek") 
                        {
                            last_msg.content.push_str(&text);
                            last_msg.is_thinking = is_thinking;
                            true
                        } else {
                            // Create new message if none exists
                            instance.add_message_with_flags(
                                "DeepSeek".to_string(),
                                text,
                                is_thinking,
                                false
                            );
                            false // add_message_with_flags already scrolls
                        };
                        
                        if should_scroll {
                            // Trigger auto-scroll after appending with stored dimensions
                            instance.auto_scroll_with_width(Some(instance.last_message_area_height), Some(instance.last_terminal_width));
                        }
                    }
                }
                DeepSeekMessage::End => {
                    tracing::info!("DeepSeek response ended");
                    
                    // Send collected response to Claude if in automode
                    if self.collecting_deepseek_response && !self.deepseek_response_buffer.is_empty() {
                        self.collecting_deepseek_response = false;
                        let full_response = self.deepseek_response_buffer.trim();
                        
                        // Extract MESSAGE_TO_CLAUDE_WITH_VERDICT and strip CoT
                        let message_to_claude = if let Some(idx) = full_response.find("MESSAGE_TO_CLAUDE_WITH_VERDICT:") {
                            let verdict_part = &full_response[idx + "MESSAGE_TO_CLAUDE_WITH_VERDICT:".len()..];
                            verdict_part.trim().to_string()
                        } else {
                            // Fallback: strip thinking sections from full response
                            Self::strip_chain_of_thought(full_response)
                        };
                        
                        if !message_to_claude.is_empty() {
                            if let Some(instance) = self.current_instance_mut() {
                                let instance_id = instance.id;
                                let session_id = instance.session_id.clone();
                                
                                // CRITICAL BUG FIX: Only send automode message if instance has session ID
                                if let Some(session_id) = session_id {
                                    let tx = self.message_tx.clone();
                                    
                                    tokio::spawn(async move {
                                        tracing::info!("Sending DeepSeek verdict to Claude: {}", message_to_claude);
                                        if let Err(e) = send_to_claude_with_session(message_to_claude, tx, Some(session_id), None).await {
                                            tracing::error!("Failed to send DeepSeek response to Claude: {}", e);
                                        }
                                    });
                                } else {
                                    tracing::warn!("⚠️  Skipping automode message - instance {} has no session ID yet", instance.name);
                                }
                            }
                        }
                    }
                }
                DeepSeekMessage::Error { error } => {
                    tracing::error!("DeepSeek error: {}", error);
                    if let Some(instance) = self.current_instance_mut() {
                        instance.add_message("DeepSeekError".to_string(), error);
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
                ClaudeMessage::StreamStart { session_id, .. } => {
                    tracing::info!("StreamStart for session {:?}", session_id);
                    // Don't create empty message - we'll create it when we get actual content
                }
                ClaudeMessage::StreamText { text, session_id } => {
                    // Debug log current state
                    tracing::info!("StreamText routing - session_id: {:?}", session_id);
                    tracing::info!("Current tab instances:");
                    for (i, inst) in self.instances.iter().enumerate() {
                        tracing::info!("  Tab {}: name={}, id={}, session={:?}, is_current={}", 
                            i, inst.name, inst.id, inst.session_id, i == self.current_tab);
                    }
                    
                    // Find instance by session_id only
                    let target_instance_index = if let Some(session_id_val) = &session_id {
                        // Find by session_id ONLY
                        let by_session = self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val));
                        if by_session.is_some() {
                            tracing::info!("Found instance by session_id: {:?}", by_session);
                        } else {
                            tracing::warn!("Session {} not found in any tab! Available sessions: {:?}", 
                                session_id_val,
                                self.instances.iter().map(|i| i.session_id.as_ref()).collect::<Vec<_>>()
                            );
                        }
                        by_session
                    } else {
                        // No session_id - route to current tab as fallback for system messages
                        tracing::info!("No session_id provided - routing system message to current tab ({})", self.current_tab);
                        Some(self.current_tab)
                    };
                    
                    // If we still can't find the instance and have a session_id, buffer the message
                    if target_instance_index.is_none() && session_id.is_some() {
                        let session_id_val = session_id.as_ref().unwrap();
                        tracing::warn!("⚠️  Failed to route StreamText: session_id={} - buffering message", session_id_val);
                        self.pending_session_messages.push((self.instance_id, text.clone(), session_id_val.clone()));
                        tracing::info!("📦 Buffered message for session {} (buffer size: {})", session_id_val, self.pending_session_messages.len());
                        continue;
                    }
                    
                    let tab_info = if let Some(session_id) = &session_id {
                        format!("Session {}", session_id)
                    } else {
                        target_instance_index
                            .map(|idx| format!("Tab {} ({})", idx + 1, self.instances[idx].name.clone()))
                            .unwrap_or_else(|| "Unknown tab".to_string())
                    };
                    
                    tracing::debug!("📝 StreamText for session ({}): {:?}", tab_info, text.chars().take(50).collect::<String>());
                    
                    // Enhanced debugging for session routing (debug level to reduce noise)
                    if session_id.is_some() {
                        let session_str = session_id.as_ref().unwrap();
                        tracing::debug!("🔍 Session routing - looking for session: {}", session_str);
                        for (i, inst) in self.instances.iter().enumerate() {
                            if let Some(ref inst_session) = inst.session_id {
                                tracing::debug!("  Tab {} ({}): session {} - {}", 
                                    i + 1, inst.name, inst_session, 
                                    if inst_session == session_str { "MATCH" } else { "no match" });
                            } else {
                                tracing::debug!("  Tab {} ({}): no session", i + 1, inst.name);
                            }
                        }
                    }
                    // Hide todo list when new output arrives
                    self.hide_todo_list();
                    
                    if let Some(instance_idx) = target_instance_index {
                        // DEBUG: Log which instance we're adding to
                        tracing::warn!("Adding message to instance_idx: {}, instance_id: {}, name: {}, is_current_tab: {}", 
                            instance_idx, 
                            self.instances[instance_idx].id,
                            self.instances[instance_idx].name,
                            instance_idx == self.current_tab
                        );
                        
                        let instance = &mut self.instances[instance_idx];
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
                            // Always trigger auto-scroll for new messages (background tabs get proper dimensions now)
                            instance.auto_scroll_with_width(Some(instance.last_message_area_height), Some(instance.last_terminal_width));
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
                            
                            // Always trigger auto-scroll after appending (background tabs get proper dimensions now)
                            instance.auto_scroll_with_width(Some(instance.last_message_area_height), Some(instance.last_terminal_width));
                            
                            // Parse todo list if needed (after releasing the mutable borrow)
                            if let Some(content) = needs_todo_parse {
                                self.parse_todo_list(&content);
                            }
                        }
                    } else {
                        // Failed to route message - could be a race condition where session hasn't been established yet
                        if let Some(ref session_id_val) = session_id {
                            tracing::warn!("⚠️  Failed to route StreamText: session_id={} - buffering message", session_id_val);
                            tracing::warn!("   Available instances: {:?}", 
                                self.instances.iter().map(|i| (i.id, i.name.clone(), i.session_id.clone())).collect::<Vec<_>>());
                            // Buffer the message for when the session gets established
                            self.pending_session_messages.push((self.instance_id, text.clone(), session_id_val.clone()));
                            tracing::info!("📦 Buffered message for session {} (buffer size: {})", session_id_val, self.pending_session_messages.len());
                        } else {
                            tracing::error!("❌ No session_id provided - cannot route or buffer message");
                            tracing::error!("   Available instances: {:?}", 
                                self.instances.iter().map(|i| (i.id, i.name.clone())).collect::<Vec<_>>());
                        }
                    }
                }
                ClaudeMessage::StreamEnd { session_id } => {
                    tracing::info!("StreamEnd for session {:?}", session_id);
                    // First, collect necessary data to avoid borrow conflicts
                    let current_tab_id = self.instances.get(self.current_tab).map(|i| i.id);
                    
                    // Find instance using session_id only
                    let target_instance_index = if let Some(session_id_val) = &session_id {
                        self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                    } else {
                        // Fallback to current tab if no session_id
                        Some(self.current_tab)
                    };
                    
                    let (claude_message_opt, main_instance_id, user_context_opt) = {
                        
                        if let Some(instance_idx) = target_instance_index {
                            let instance = &mut self.instances[instance_idx];
                            instance.is_processing = false;
                            
                            // Track successful tool usage to avoid unnecessary permission checks
                            if !instance.last_tool_attempts.is_empty() {
                                // If we completed successfully after tool attempts, those tools must have worked
                                for tool in &instance.last_tool_attempts {
                                    if !instance.successful_tools.contains(tool) {
                                        instance.successful_tools.push(tool.clone());
                                        tracing::info!("Marking tool '{}' as successfully used for session {:?}", tool, session_id);
                                    }
                                }
                            }
                            
                            // Check if this is the current tab and process queue
                            let _is_current_tab = target_instance_index.map(|idx| idx == self.current_tab).unwrap_or(false);
                            
                            // Process with automode if enabled
                            if self.auto_mode {
                                tracing::info!("Automode is ON, checking last message");
                                if let Some(last_msg) = instance.messages.last() {
                                    tracing::info!("Last message sender: {}, content length: {}", last_msg.sender, last_msg.content.len());
                                    if last_msg.sender == "Claude" && !last_msg.content.is_empty() {
                                        let claude_message = last_msg.content.clone();
                                        let main_instance_id = instance.id;
                                        
                                        // Get user context from previous messages
                                        let user_context = instance.messages.iter()
                                            .rev()
                                            .find(|m| m.sender == "You")
                                            .map(|m| m.content.clone())
                                            .unwrap_or_default();
                                        
                                        (Some(claude_message), main_instance_id, Some(user_context))
                                    } else {
                                        (None, instance.id, None)
                                    }
                                } else {
                                    (None, instance.id, None)
                                }
                            } else {
                                (None, instance.id, None)
                            }
                        } else {
                            (None, uuid::Uuid::new_v4(), None)
                        }
                    };
                    
                    // Now handle coordination analysis without borrowing conflicts
                    if let (Some(ref claude_message), Some(ref user_context)) = (claude_message_opt, user_context_opt) {
                        tracing::info!("Processing StreamEnd - coordination enabled: {}, current instances: {}, max: {}", 
                                      self.coordination_enabled, self.instances.len(), self.max_instances);
                        
                        // Check if this task would benefit from coordination (only if not already coordinating)
                        if !self.coordination_in_progress {
                            if let Some(instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                                instance.add_message("System".to_string(), 
                                    "🤖 Analyzing if task would benefit from multi-instance coordination...".to_string());
                            }
                            
                            if self.analyze_task_for_coordination(&claude_message).await {
                                tracing::info!("Task identified for multi-instance coordination");
                                
                                // Set coordination in progress to prevent stall detection interference
                                self.coordination_in_progress = true;
                            
                            // Set a safety timeout to clear coordination flag in case something goes wrong
                            let tx_safety = self.message_tx.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(tokio::time::Duration::from_secs(300)).await; // 5 minutes safety timeout
                                tracing::warn!("Coordination timeout - clearing coordination_in_progress flag");
                                // Send a dummy message to trigger flag clearing if needed
                                let _ = tx_safety.send(ClaudeMessage::InternalCoordinateInstances {
                                    main_instance_id: uuid::Uuid::new_v4(),
                                    task_description: "TIMEOUT: Coordination safety timeout triggered".to_string(),
                                    num_instances: 0,
                                    working_dir: ".".to_string(),
                                    is_ipc: false,
                                }).await;
                            });
                            
                            // Clone necessary data for the background task
                            let task_desc_clone = claude_message.clone();
                            let current_dir = if let Some(instance) = self.instances.iter().find(|i| i.id == main_instance_id) {
                                instance.working_directory.clone()
                            } else {
                                std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".to_string())
                            };
                            let tx = self.message_tx.clone();
                            
                            // Show processing message
                            if let Some(instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                                instance.add_message("System".to_string(), 
                                    "⏳ Analyzing task for multi-instance coordination...".to_string());
                            }
                            
                            // Spawn coordination in background
                            tokio::spawn(async move {
                                tracing::info!("Starting background coordination analysis");
                                
                                // Perform DeepSeek analysis in background
                                let breakdown_prompt = format!(
                                    r#"Break down this complex task into 2-3 parallel subtasks that can be worked on by separate Claude Code instances:

Main task: "{}"
Working directory: {}

Requirements:
1. Each subtask should be independent and workable in parallel
2. Subtasks should be specific and actionable
3. Include file/directory scope for each subtask to avoid conflicts
4. Ensure subtasks contribute to the overall goal

Format your response as:
SUBTASK_1: [Description] | SCOPE: [Files/directories] | PRIORITY: [High/Medium/Low]
SUBTASK_2: [Description] | SCOPE: [Files/directories] | PRIORITY: [High/Medium/Low]  
SUBTASK_3: [Description] | SCOPE: [Files/directories] | PRIORITY: [High/Medium/Low]

Response:"#,
                                    task_desc_clone,
                                    current_dir
                                );
                                
                                // Perform the analysis (this might take time but won't block UI)
                                match perform_gemma_analysis(&breakdown_prompt).await {
                                    Ok(breakdown) => {
                                        tracing::info!("Auto-coordination analysis completed, sending InternalCoordinateInstances message");
                                        if let Err(e) = tx.send(ClaudeMessage::InternalCoordinateInstances {
                                            main_instance_id,
                                            task_description: breakdown,
                                            num_instances: 0, // Use default count for auto-coordination
                                            working_dir: current_dir,
                                            is_ipc: false,
                                        }).await {
                                            tracing::error!("Failed to send auto-coordination InternalCoordinateInstances message: {}", e);
                                        } else {
                                            tracing::info!("Successfully sent auto-coordination InternalCoordinateInstances message");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Background auto-coordination failed: {}", e);
                                        if let Err(e2) = tx.send(ClaudeMessage::InternalCoordinateInstances {
                                            main_instance_id,
                                            task_description: "ERROR: Failed to analyze task - using single instance".to_string(),
                                            num_instances: 0,
                                            working_dir: current_dir,
                                            is_ipc: false,
                                        }).await {
                                            tracing::error!("Failed to send auto-coordination fallback message: {}", e2);
                                        } else {
                                            tracing::info!("Successfully sent auto-coordination fallback message");
                                        }
                                    }
                                }
                            });
                            
                            return; // Don't continue with normal automode processing
                        }
                        } else {
                            tracing::debug!("Coordination already in progress, skipping automode coordination analysis");
                        }
                        
                        // Continue with normal automode processing - collect more data
                        let (had_tool_attempts, attempted_tools, session_id_opt) = {
                            if let Some(instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                                let had_tool_attempts = !instance.last_tool_attempts.is_empty();
                                
                                // Filter out tools that we know were successful - no need to check permission
                                let attempted_tools: Vec<String> = instance.last_tool_attempts.iter()
                                    .filter(|tool| !instance.successful_tools.contains(tool))
                                    .cloned()
                                    .collect();
                                
                                let skipped_tools: Vec<String> = instance.last_tool_attempts.iter()
                                    .filter(|tool| instance.successful_tools.contains(tool))
                                    .cloned()
                                    .collect();
                                
                                // Clear tool attempts for next message
                                instance.last_tool_attempts.clear();
                                
                                // Add system message if tools were attempted
                                if had_tool_attempts {
                                    if !attempted_tools.is_empty() {
                                        instance.add_message("System".to_string(), 
                                            format!("🤖 Automode: Checking if Claude needs permission for tools: {}", attempted_tools.join(", ")));
                                    }
                                    if !skipped_tools.is_empty() {
                                        instance.add_message("System".to_string(), 
                                            format!("✅ Automode: Skipping permission check for proven tools: {}", skipped_tools.join(", ")));
                                    }
                                }
                                
                                (had_tool_attempts && !attempted_tools.is_empty(), attempted_tools, instance.session_id.clone())
                            } else {
                                (false, Vec::new(), None)
                            }
                        };
                        
                        // Check if automode is enabled before processing
                        if self.auto_mode {
                            if let Some(session_id) = session_id_opt {
                                let tx = self.message_tx.clone();
                                let deepseek_tx = self.deepseek_tx.clone();
                                let claude_msg_for_permission = claude_message.clone();
                                let user_context_for_spawn = user_context.clone();
                                        
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
                                                        text: system_msg,
                                                        session_id: Some(session_id.clone()),
                                                    }).await;
                                                    
                                                    // Send a message telling Claude the tools are now enabled
                                                    let response = format!(
                                                        "I've enabled the following tools for you: {}. Please try using them again.",
                                                        enabled_tools.join(", ")
                                                    );
                                                    
                                                    if let Err(e) = send_to_claude_with_session(response, tx, Some(session_id), None).await {
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
                                        // No tool attempts, check if Claude is requesting coordination
                                        let message_lower = claude_msg_for_permission.to_lowercase();
                                        let coordination_requests = [
                                            "spawn additional instances",
                                            "multiple instances", 
                                            "parallel processing",
                                            "divide and conquer",
                                            "coordinate with other instances",
                                            "split this task",
                                            "work in parallel",
                                            "I should spawn",
                                            "let me spawn",
                                            "I need additional instances"
                                        ];
                                        
                                        let mut coordination_requested = false;
                                        for request in &coordination_requests {
                                            if message_lower.contains(request) {
                                                tracing::info!("Claude explicitly requested coordination: '{}'", request);
                                                coordination_requested = true;
                                                break;
                                            }
                                        }
                                        
                                        if coordination_requested {
                                            // Send a message asking for user confirmation for coordination
                                            let coordination_response = "I can spawn additional Claude instances to work on this task in parallel. Would you like me to proceed with multi-instance coordination?";
                                            if let Err(e) = send_to_claude_with_session(coordination_response.to_string(), tx.clone(), Some(session_id.clone()), None).await {
                                                tracing::error!("Failed to send coordination query: {}", e);
                                            }
                                        } else {
                                            // Check if it's a regular question
                                            let (is_question, _) = analyze_claude_message(&claude_msg_for_permission);
                                            
                                            if is_question {
                                                tracing::info!("Automode: Claude asked a question, generating DeepSeek response");
                                                
                                                // Generate streaming response for UI display
                                                tokio::spawn(async move {
                                                    if let Err(e) = generate_deepseek_response_stream(
                                                        &claude_msg_for_permission, 
                                                        &user_context_for_spawn,
                                                        deepseek_tx
                                                    ).await {
                                                        tracing::error!("Failed to generate DeepSeek response: {}", e);
                                                    }
                                                });
                                            }
                                        }
                                    }
                                });
                            } else {
                                tracing::warn!("No session_id available for automode processing");
                            }
                        } else {
                            tracing::info!("Automode is OFF");
                        }
                    } else {
                        // No claude message or user context for automode processing
                        tracing::debug!("No claude message or user context available for automode");
                    }
                    
                    // Process message queue if this is the current tab and instance finished processing
                    if let Some(target_idx) = target_instance_index {
                        if target_idx == self.current_tab && !self.message_queue.is_empty() {
                            tracing::info!("Instance finished processing, checking message queue ({} messages)", self.message_queue.len());
                            self.process_message_queue().await;
                        }
                    }
                }
                ClaudeMessage::Error { error, session_id } => {
                    tracing::error!("Error for session {:?}: {}", session_id, error);
                    // Find instance using session_id only
                    let target_instance_index = if let Some(session_id_val) = &session_id {
                        self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                    } else {
                        Some(self.current_tab)
                    };
                    
                    if let Some(instance_idx) = target_instance_index {
                        let instance = &mut self.instances[instance_idx];
                        instance.add_message("Error".to_string(), error);
                        instance.is_processing = false;
                    }
                    
                    // Process message queue if this is the current tab
                    if let Some(target_idx) = target_instance_index {
                        if target_idx == self.current_tab && !self.message_queue.is_empty() {
                            tracing::info!("Instance had error, checking message queue ({} messages)", self.message_queue.len());
                            self.process_message_queue().await;
                        }
                    }
                }
                ClaudeMessage::Exited { code, session_id } => {
                    tracing::info!("Process exited for session {:?} with code: {:?}", session_id, code);
                    
                    // Find instance using session_id only
                    let target_instance_index = if let Some(session_id_val) = &session_id {
                        self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                    } else {
                        Some(self.current_tab)
                    };
                    
                    if let Some(instance_idx) = target_instance_index {
                        self.instances[instance_idx].is_processing = false;
                    }
                    
                    // Process message queue if this is the current tab
                    if let Some(target_idx) = target_instance_index {
                        if target_idx == self.current_tab && !self.message_queue.is_empty() {
                            tracing::info!("Instance exited, checking message queue ({} messages)", self.message_queue.len());
                            self.process_message_queue().await;
                        }
                    }
                }
                ClaudeMessage::ToolUse { tool_name, session_id } => {
                    tracing::info!("Tool use attempt for session {:?}: {}", session_id, tool_name);
                    
                    // Show todo list if TodoRead or TodoWrite is used
                    if tool_name == "TodoRead" || tool_name == "TodoWrite" {
                        self.show_todo_list();
                    }
                    
                    // Find instance using session_id only
                    let target_instance_index = if let Some(session_id_val) = &session_id {
                        self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                    } else {
                        Some(self.current_tab)
                    };
                    
                    if let Some(instance_idx) = target_instance_index {
                        let instance = &mut self.instances[instance_idx];
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
                ClaudeMessage::SessionStarted { session_id } => {
                    tracing::info!("🎬 SessionStarted received for session_id: {}", session_id);
                    
                    // No target specified - find first instance without a session_id
                    let target_instance_index = self.instances.iter().position(|i| i.session_id.is_none());
                    tracing::info!("🔄 No target specified, using fallback assignment: {:?}", target_instance_index);
                    
                    self.assign_session_to_instance(target_instance_index, session_id);
                }
                ClaudeMessage::ToolPermissionDenied { tool_name, session_id, .. } => {
                    tracing::info!("Tool permission denied for session {:?}: {}", session_id, tool_name);
                    
                    // Find instance by session_id
                    let target_instance_index = if let Some(session_id_val) = &session_id {
                        self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                    } else {
                        // Fallback to first instance if no session_id
                        Some(0)
                    };
                    
                    if let Some(index) = target_instance_index {
                        let instance = &mut self.instances[index];
                        // Remove tool from successful list since it was explicitly denied
                        instance.successful_tools.retain(|t| t != &tool_name);
                        tracing::info!("Removed tool '{}' from successful list due to permission denial", tool_name);
                        
                        instance.add_message("System".to_string(), format!("🔒 Permission denied for tool: {}", tool_name));
                        
                        // In automode, ask DeepSeek to analyze if this tool should be enabled
                        if self.auto_mode {
                            let tool_name_copy = tool_name.clone();
                            let session_id_copy = instance.session_id.clone();
                            let process_handle = instance.process_handle.clone();
                            let tx = self.message_tx.clone();
                            
                            tokio::spawn(async move {
                                tracing::info!("Automode: Analyzing safety of tool: {}", tool_name_copy);
                                
                                match Self::analyze_tool_safety(&tool_name_copy).await {
                                    Ok(true) => {
                                        tracing::info!("DeepSeek approved enabling tool: {}", tool_name_copy);
                                        
                                        // Enable the tool
                                        if let Err(e) = enable_claude_tool(&tool_name_copy).await {
                                            tracing::error!("Failed to enable tool {}: {}", tool_name_copy, e);
                                            let _ = tx.send(ClaudeMessage::StreamText {
                                                text: format!("❌ Failed to enable {}: {}", tool_name_copy, e),
                                                session_id: session_id_copy.clone(),
                                            }).await;
                                        } else {
                                            tracing::info!("Successfully enabled tool: {}", tool_name_copy);
                                            let _ = tx.send(ClaudeMessage::StreamText {
                                                text: format!("🔧 Automode: Safely enabled tool: {}", tool_name_copy),
                                                session_id: session_id_copy.clone(),
                                            }).await;
                                            
                                            // Kill the current process if it exists
                                            let killed_process = if let Some(handle) = process_handle.clone() {
                                                let mut handle_guard = handle.lock().await;
                                                if let Some(ref mut child) = *handle_guard {
                                                    #[cfg(unix)]
                                                    {
                                                        use nix::sys::signal::{self, Signal};
                                                        use nix::unistd::Pid;
                                                        
                                                        if let Some(pid) = child.id() {
                                                            tracing::info!("Tool enablement: Killing Claude process {} for session {:?}", pid, session_id_copy);
                                                            // First try SIGINT
                                                            match signal::kill(Pid::from_raw(pid as i32), Signal::SIGINT) {
                                                                Ok(_) => tracing::info!("Sent SIGINT to process {}", pid),
                                                                Err(e) => tracing::error!("Failed to send SIGINT to {}: {}", pid, e),
                                                            }
                                                            
                                                            // Wait for process to exit gracefully
                                                            let mut waited = 0;
                                                            while waited < 2000 { // Wait up to 2 seconds
                                                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                                                match child.try_wait() {
                                                                    Ok(Some(_)) => {
                                                                        tracing::info!("Process {} terminated gracefully", pid);
                                                                        break;
                                                                    }
                                                                    Ok(None) => {
                                                                        waited += 100;
                                                                    }
                                                                    Err(e) => {
                                                                        tracing::error!("Error waiting for process: {}", e);
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                            
                                                            // If still running, force kill
                                                            if waited >= 2000 {
                                                                tracing::warn!("Process {} didn't respond to SIGINT, using SIGKILL", pid);
                                                                let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
                                                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                                            }
                                                        }
                                                    }
                                                    
                                                    #[cfg(not(unix))]
                                                    {
                                                        // On Windows, just kill the process
                                                        let _ = child.kill().await;
                                                    }
                                                }
                                                // Clear the handle since we're killing the process
                                                *handle_guard = None;
                                                drop(handle_guard);
                                                true
                                            } else {
                                                false
                                            };
                                            
                                            if killed_process {
                                                // Wait a bit more to ensure process is fully terminated
                                                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                                            }
                                            
                                            // Resume the session with tool enablement message
                                            let response = format!("I've enabled the {} tool for you. Please try using it again.", tool_name_copy);
                                            
                                            // Create a new process handle for the resumed session
                                            let new_handle = Arc::new(tokio::sync::Mutex::new(None));
                                            
                                            // Update the instance's process handle
                                            let _ = tx.send(ClaudeMessage::StreamText {
                                                text: format!("📝 Resuming session after enabling tool: {}", tool_name_copy),
                                                session_id: session_id_copy.clone(),
                                            }).await;
                                            
                                            tracing::info!("Resuming session {:?} with tool {} enabled", session_id_copy, tool_name_copy);
                                            if let Err(e) = send_to_claude_with_session(response, tx.clone(), session_id_copy.clone(), Some(new_handle.clone())).await {
                                                tracing::error!("Failed to resume session {:?} with tool enablement: {}", session_id_copy, e);
                                            } else {
                                                tracing::info!("Successfully initiated session resume for {:?} with tool {} enabled", session_id_copy, tool_name_copy);
                                            }
                                        }
                                    }
                                    Ok(false) => {
                                        tracing::warn!("DeepSeek determined tool {} is unsafe to enable", tool_name_copy);
                                        let _ = tx.send(ClaudeMessage::StreamText {
                                            text: format!("🚫 Automode: Tool {} was deemed unsafe and not enabled", tool_name_copy),
                                            session_id: session_id_copy.clone(),
                                        }).await;
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to analyze tool safety: {}", e);
                                        let _ = tx.send(ClaudeMessage::StreamText {
                                            text: format!("⚠️ Could not analyze safety of tool {}: {}", tool_name_copy, e),
                                            session_id: session_id_copy.clone(),
                                        }).await;
                                    }
                                }
                            });
                        }
                    }
                }
                ClaudeMessage::VedaSpawnInstances { instance_id, task_description, num_instances } => {
                    tracing::info!("Claude requested to spawn {} instances for task: {}", num_instances, task_description);
                    
                    // Check if it's from IPC (not from a real instance)
                    let is_ipc = self.instances.iter().find(|i| i.id == instance_id).is_none();
                    
                    if !is_ipc {
                        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                            instance.add_message("Tool".to_string(), 
                                format!("🤝 Spawning {} additional instances for task: {}", num_instances, task_description));
                        }
                    }
                    
                    // Use the first instance for coordination if IPC request
                    let coord_instance_id = if is_ipc && !self.instances.is_empty() {
                        self.instances[0].id
                    } else {
                        instance_id
                    };
                    
                    // Clone necessary data for the background task
                    let task_desc_clone = task_description.clone();
                    let num_instances_clone = num_instances;
                    let current_dir = if let Some(instance) = self.instances.iter().find(|i| i.id == coord_instance_id) {
                        instance.working_directory.clone()
                    } else {
                        std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".to_string())
                    };
                    let tx = self.message_tx.clone();
                    
                    // Set coordination in progress to prevent stall detection interference
                    self.coordination_in_progress = true;
                    
                    // Set a safety timeout to clear coordination flag in case something goes wrong
                    let tx_safety = self.message_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_secs(300)).await; // 5 minutes safety timeout
                        tracing::warn!("Direct spawn coordination timeout - clearing coordination_in_progress flag");
                        // Send a dummy message to trigger flag clearing if needed
                        let _ = tx_safety.send(ClaudeMessage::InternalCoordinateInstances {
                            main_instance_id: uuid::Uuid::new_v4(),
                            task_description: "TIMEOUT: Direct spawn coordination safety timeout triggered".to_string(),
                            num_instances: 0,
                            working_dir: ".".to_string(),
                            is_ipc: false,
                        }).await;
                    });
                    
                    // Immediately show that we're processing the request
                    if is_ipc && !self.instances.is_empty() {
                        self.instances[0].add_message("System".to_string(), 
                            format!("⏳ Processing request to spawn {} instances...", num_instances));
                    }
                    
                    // Spawn coordination in background to avoid blocking UI
                    tokio::spawn(async move {
                        tracing::info!("Starting background coordination for {} instances", num_instances_clone);
                        
                        // Perform DeepSeek analysis in background
                        let breakdown_prompt = format!(
                            r#"Break down this complex task into 2-3 parallel subtasks that can be worked on by separate Claude Code instances:

Main task: "{}"
Working directory: {}

Requirements:
1. Each subtask should be independent and workable in parallel
2. Subtasks should be specific and actionable
3. Include file/directory scope for each subtask to avoid conflicts
4. Ensure subtasks contribute to the overall goal

Format your response as:
SUBTASK_1: [Description] | SCOPE: [Files/directories] | PRIORITY: [High/Medium/Low]
SUBTASK_2: [Description] | SCOPE: [Files/directories] | PRIORITY: [High/Medium/Low]  
SUBTASK_3: [Description] | SCOPE: [Files/directories] | PRIORITY: [High/Medium/Low]

Response:"#,
                            task_desc_clone,
                            current_dir
                        );
                        
                        // Perform the analysis with reasonable timeout for Gemma3:12b (much faster than DeepSeek)
                        let analysis_timeout = tokio::time::Duration::from_secs(60); // 1 minute max for coordination
                        match tokio::time::timeout(analysis_timeout, perform_gemma_analysis(&breakdown_prompt)).await {
                            Ok(Ok(breakdown)) => {
                                tracing::info!("Background analysis completed, sending InternalCoordinateInstances message");
                                if let Err(e) = tx.send(ClaudeMessage::InternalCoordinateInstances {
                                    main_instance_id: coord_instance_id,
                                    task_description: breakdown,
                                    num_instances: num_instances_clone as usize,
                                    working_dir: current_dir,
                                    is_ipc,
                                }).await {
                                    tracing::error!("Failed to send InternalCoordinateInstances message: {}", e);
                                } else {
                                    tracing::info!("Successfully sent InternalCoordinateInstances message");
                                }
                            }
                            Ok(Err(e)) => {
                                tracing::error!("Background coordination analysis error: {}", e);
                                // Send fallback coordination message with intelligent task breakdown
                                let fallback_breakdown = format!(
                                    "SUBTASK_1: {} | SCOPE: Main project files | PRIORITY: High\nSUBTASK_2: Testing and validation | SCOPE: tests/ | PRIORITY: Medium\nSUBTASK_3: Documentation and cleanup | SCOPE: docs/, README.md | PRIORITY: Low", 
                                    task_desc_clone
                                );
                                if let Err(e) = tx.send(ClaudeMessage::InternalCoordinateInstances {
                                    main_instance_id: coord_instance_id,
                                    task_description: fallback_breakdown,
                                    num_instances: num_instances_clone as usize,
                                    working_dir: current_dir,
                                    is_ipc,
                                }).await {
                                    tracing::error!("Failed to send fallback InternalCoordinateInstances message: {}", e);
                                } else {
                                    tracing::info!("Successfully sent fallback InternalCoordinateInstances message after analysis error");
                                }
                            }
                            Err(_) => {
                                tracing::error!("Gemma coordination analysis timed out after 1 minute");
                                // Send fallback coordination message with intelligent task breakdown
                                let fallback_breakdown = format!(
                                    "SUBTASK_1: {} | SCOPE: Main project files | PRIORITY: High\nSUBTASK_2: Testing and validation | SCOPE: tests/ | PRIORITY: Medium\nSUBTASK_3: Documentation and cleanup | SCOPE: docs/, README.md | PRIORITY: Low", 
                                    task_desc_clone
                                );
                                if let Err(e) = tx.send(ClaudeMessage::InternalCoordinateInstances {
                                    main_instance_id: coord_instance_id,
                                    task_description: fallback_breakdown,
                                    num_instances: num_instances_clone as usize,
                                    working_dir: current_dir,
                                    is_ipc,
                                }).await {
                                    tracing::error!("Failed to send fallback InternalCoordinateInstances message: {}", e);
                                } else {
                                    tracing::info!("Successfully sent fallback InternalCoordinateInstances message after Gemma timeout");
                                }
                            }
                        }
                    });
                }
                ClaudeMessage::VedaListInstances { instance_id } => {
                    tracing::info!("Claude requested instance list");
                    
                    // Check if it's from IPC
                    let is_ipc = self.instances.iter().find(|i| i.id == instance_id).is_none();
                    
                    // Collect instance information first
                    let mut instance_info = Vec::new();
                    instance_info.push("📋 Current Claude Instances:".to_string());
                    
                    for (i, inst) in self.instances.iter().enumerate() {
                        let status = if inst.is_processing { "(Processing)" } else { "(Idle)" };
                        let current_marker = if i == self.current_tab { " ← Current" } else { "" };
                        instance_info.push(format!("  {}. {} {} - Dir: {}{}", 
                            i + 1, inst.name, status, inst.working_directory, current_marker));
                    }
                    
                    let message = instance_info.join("\n");
                    
                    // Send to appropriate instance
                    if is_ipc && !self.instances.is_empty() {
                        self.instances[0].add_message("System".to_string(), message);
                    } else if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        instance.add_message("Tool".to_string(), message);
                    }
                }
                ClaudeMessage::VedaCloseInstance { instance_id, target_instance_name } => {
                    tracing::info!("Claude requested to close instance: {}", target_instance_name);
                    
                    // Find the target instance by name and collect necessary info
                    let target_index = self.instances.iter().position(|inst| inst.name == target_instance_name);
                    let instances_len = self.instances.len();
                    
                    let result_message = if let Some(target_idx) = target_index {
                        if target_idx == 0 {
                            "❌ Cannot close the main instance (Claude 1)".to_string()
                        } else if instances_len <= 1 {
                            "❌ Cannot close the last remaining instance".to_string()
                        } else {
                            let closed_name = self.instances[target_idx].name.clone();
                            self.instances.remove(target_idx);
                            
                            // Adjust current tab if necessary
                            if self.current_tab >= self.instances.len() {
                                self.current_tab = self.instances.len() - 1;
                            } else if self.current_tab > target_idx {
                                self.current_tab -= 1;
                            }
                            
                            self.sync_working_directory();
                            format!("✅ Closed instance: {}", closed_name)
                        }
                    } else {
                        format!("❌ Instance '{}' not found", target_instance_name)
                    };
                    
                    // Send the result message
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        instance.add_message("Tool".to_string(), result_message);
                    }
                }
                ClaudeMessage::InternalCoordinateInstances { main_instance_id, task_description, num_instances, working_dir, is_ipc } => {
                    tracing::info!("Processing background coordination for {} instances", num_instances);
                    
                    // Spawn instances directly without further DeepSeek analysis
                    self.spawn_coordinated_instances_with_count(main_instance_id, &task_description, &working_dir, num_instances).await;
                    
                    // Clear coordination in progress flag
                    self.coordination_in_progress = false;
                    
                    // Send completion message
                    if is_ipc && !self.instances.is_empty() {
                        self.instances[0].add_message("System".to_string(), 
                            format!("✅ Completed spawning {} instances for task", num_instances));
                    }
                }
                ClaudeMessage::CoordinationMessage { message } => {
                    tracing::info!("Received coordination message: {:?}", message);
                    // Handle inter-Veda coordination messages
                    // This is for future multi-Veda coordination functionality
                    // For now, just log the message
                    if !self.instances.is_empty() {
                        self.instances[0].add_message("Coordination".to_string(), 
                            format!("📡 Coordination: {} -> {}: {}", 
                                message.from, 
                                message.to.as_deref().unwrap_or("broadcast"),
                                message.summary));
                    }
                }
            }
        }
    }

    async fn check_for_stalls(&mut self) {
        if !self.auto_mode {
            return; // Only check for stalls in automode
        }
        
        if self.coordination_in_progress {
            // Rate limit this log message to once every 60 seconds
            let now = std::time::Instant::now();
            let should_log = match self.last_coordination_skip_log {
                None => true,
                Some(last_log) => now.duration_since(last_log).as_secs() >= 60,
            };
            
            if should_log {
                tracing::debug!("Skipping stall detection - coordination in progress");
                self.last_coordination_skip_log = Some(now);
            }
            return; // Don't check for stalls while coordination is in progress
        }
        
        // First check if we should trigger stall detection and get context
        let stall_info = if let Some(instance) = self.current_instance_mut() {
            if instance.should_check_for_stall() {
                let delay_seconds = instance.stall_delay_seconds;
                let instance_id = instance.id;
                let (claude_message, user_context) = instance.get_recent_context();
                
                // Mark that we've sent a stall check and intervention is in progress
                instance.stall_check_sent = true;
                instance.stall_intervention_in_progress = true;
                
                Some((delay_seconds, instance_id, claude_message, user_context))
            } else {
                None
            }
        } else {
            None
        };
        
        // If we have stall info, process it
        if let Some((delay_seconds, instance_id, claude_message, user_context)) = stall_info {
            tracing::info!("Detected conversation stall for instance {} after {} seconds, triggering DeepSeek intervention", 
                          instance_id, delay_seconds);
            
            // Clone the sender before the mutable borrow
            let deepseek_tx = self.deepseek_tx.clone();
            
            // Add system message about stall detection
            if let Some(instance) = self.current_instance_mut() {
                instance.add_message("System".to_string(), 
                    format!("🕐 Conversation stalled ({}s) - DeepSeek analyzing...", delay_seconds));
            }
            
            // Add a timeout to prevent infinite stall intervention
            let intervention_timeout = tokio::time::Duration::from_secs(60); // 1 minute timeout
            
            tokio::spawn(async move {
                tracing::info!("Generating stall intervention response");
                let result = tokio::time::timeout(
                    intervention_timeout,
                    generate_deepseek_stall_response(&claude_message, &user_context, deepseek_tx)
                ).await;
                
                match result {
                    Ok(Ok(())) => {
                        tracing::info!("Stall intervention completed successfully");
                    }
                    Ok(Err(e)) => {
                        tracing::error!("Failed to generate stall response: {}", e);
                    }
                    Err(_) => {
                        tracing::error!("Stall intervention timed out after 60 seconds");
                    }
                }
            });
        }
    }

    async fn analyze_task_for_coordination(&mut self, claude_message: &str) -> bool {
        if !self.coordination_enabled {
            tracing::debug!("Coordination disabled, skipping analysis");
            return false;
        }
        
        if self.instances.len() >= self.max_instances {
            tracing::debug!("Already at max instances ({}), skipping coordination", self.max_instances);
            return false;
        }
        
        // Check for explicit coordination requests first
        let explicit_keywords = [
            "spawn additional instances",
            "multiple instances", 
            "parallel processing",
            "divide and conquer",
            "coordinate with other instances",
            "split this task",
            "work in parallel"
        ];
        
        let message_lower = claude_message.to_lowercase();
        for keyword in &explicit_keywords {
            if message_lower.contains(keyword) {
                tracing::info!("Explicit coordination request detected: '{}'", keyword);
                return true;
            }
        }
        
        // Use DeepSeek to analyze if task would benefit from multiple instances
        tracing::info!("Analyzing coordination potential with DeepSeek for message length: {}", claude_message.len());
        
        let analysis_prompt = format!(
            r#"Analyze if this Claude message indicates a task that would benefit from multiple parallel Claude Code instances working together:

Claude's message: "{}"

Consider these factors for PARALLEL INSTANCES (respond COORDINATE_BENEFICIAL if ANY apply):
1. Multiple independent components/modules that can be worked on separately
2. Multiple separate features that can be developed in parallel  
3. Tasks like "implement X, Y, and Z" where X, Y, Z are separable and independent
4. Testing multiple components simultaneously without interference
5. Documentation generation across multiple independent areas
6. Refactoring that can be divided by file/module boundaries
7. Claude mentions working on multiple files/directories
8. Task involves parallel development streams

IMPORTANT: Independent, separable tasks are IDEAL for parallel instances!

Respond with EXACTLY one of:
COORDINATE_BENEFICIAL: [Brief reason - focus on independence and separability]
SINGLE_INSTANCE_SUFFICIENT: [Brief reason - only if tasks are tightly coupled/interdependent]

Your response:"#,
            claude_message
        );
        
        // Quick local analysis using DeepSeek with timeout protection
        let analysis_timeout = tokio::time::Duration::from_secs(60); // Allow up to 60 seconds for analysis
        match tokio::time::timeout(analysis_timeout, self.quick_deepseek_analysis(&analysis_prompt)).await {
            Ok(Ok(response)) => {
                tracing::info!("DeepSeek coordination analysis response: {}", response);
                if response.contains("COORDINATE_BENEFICIAL") {
                    tracing::info!("DeepSeek recommends coordination for task");
                    true
                } else {
                    tracing::debug!("DeepSeek says single instance sufficient: {}", response);
                    false
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("DeepSeek analysis failed: {}, skipping coordination", e);
                false
            }
            Err(_) => {
                tracing::warn!("DeepSeek analysis timed out after 60s, skipping coordination");
                false
            }
        }
    }
    
    
    async fn quick_deepseek_analysis(&self, prompt: &str) -> Result<String> {
        let request_body = serde_json::json!({
            "model": "gemma3:12b",
            "prompt": prompt,
            "stream": false
        });
        
        let client = reqwest::Client::new();
        
        // Retry up to 10 times with exponential backoff
        let mut retry_count = 0;
        let max_retries = 10;
        
        loop {
            match client
                .post("http://localhost:11434/api/generate")
                .json(&request_body)
                .timeout(Duration::from_secs(120))
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        #[derive(serde::Deserialize)]
                        struct OllamaResponse {
                            response: String,
                        }
                        
                        match response.json::<OllamaResponse>().await {
                            Ok(ollama_response) => {
                                return Ok(ollama_response.response.trim().to_string());
                            }
                            Err(e) => {
                                tracing::error!("Failed to parse Ollama response: {}", e);
                                return Err(anyhow::anyhow!("Failed to parse Ollama response: {}", e));
                            }
                        }
                    } else {
                        tracing::warn!("Ollama API error: status {}", response.status());
                        if retry_count >= max_retries {
                            return Err(anyhow::anyhow!("Ollama API error after {} retries", max_retries));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to contact Ollama (attempt {}/{}): {}", retry_count + 1, max_retries, e);
                    if retry_count >= max_retries {
                        return Err(anyhow::anyhow!("Failed to contact Ollama after {} retries: {}", max_retries, e));
                    }
                }
            }
            
            // Exponential backoff: 1s, 2s, 4s
            retry_count += 1;
            let delay_secs = 1u64 << (retry_count - 1);
            tracing::info!("Retrying Ollama request in {} seconds...", delay_secs);
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        }
    }
    
    async fn coordinate_multi_instance_task(&mut self, main_instance_id: Uuid, task_description: &str) {
        // Use the default coordination logic
        self.coordinate_multi_instance_task_with_count(main_instance_id, task_description, 0).await;
    }
    
    async fn coordinate_multi_instance_task_with_count(&mut self, main_instance_id: Uuid, task_description: &str, requested_count: usize) {
        tracing::info!("Coordinating multi-instance task: {}", task_description);
        
        // Get current working directory
        let current_dir = if let Some(instance) = self.instances.iter().find(|i| i.id == main_instance_id) {
            instance.working_directory.clone()
        } else {
            std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".to_string())
        };
        
        // This function is now only called from the background task with processed breakdown
        // The DeepSeek analysis happens in the background, not here
        self.spawn_coordinated_instances_with_count(main_instance_id, task_description, &current_dir, requested_count).await;
    }
    
    async fn spawn_coordinated_instances(&mut self, main_instance_id: Uuid, breakdown: &str, working_dir: &str) {
        self.spawn_coordinated_instances_with_count(main_instance_id, breakdown, working_dir, 0).await;
    }
    
    async fn spawn_coordinated_instances_with_count(&mut self, main_instance_id: Uuid, breakdown: &str, working_dir: &str, requested_count: usize) {
        let subtasks: Vec<&str> = breakdown.lines()
            .filter(|line| line.starts_with("SUBTASK_"))
            .collect();
        
        // Log the breakdown to understand why subtasks are empty
        tracing::warn!("Gemma breakdown analysis result: {:?}", breakdown);
        tracing::warn!("Extracted subtasks: {:?}", subtasks);
        
        if subtasks.is_empty() {
            tracing::warn!("No subtasks found in breakdown, attempting to parse it better for Gemma guidance");
            
            // Try to extract meaningful task info even from failed Gemma analysis
            let task_info = if breakdown.starts_with("ERROR:") {
                "General development tasks"
            } else if breakdown.contains("Parallel task execution requested") {
                breakdown.split(':').last().unwrap_or(breakdown).trim()
            } else {
                breakdown
            };
            
            // Instead of generic instances, create structured fallback subtasks
            let fallback_subtasks = vec![
                format!("SUBTASK_1: {} - Core implementation | SCOPE: Main source files | PRIORITY: High", task_info),
                format!("SUBTASK_2: {} - Testing and validation | SCOPE: tests/ directory | PRIORITY: Medium", task_info),
                format!("SUBTASK_3: {} - Documentation and integration | SCOPE: docs/, README.md | PRIORITY: Low", task_info),
            ];
            
            if requested_count > 0 {
                for i in 0..requested_count.min(self.max_instances - self.instances.len()).min(fallback_subtasks.len()) {
                    let subtask_line = &fallback_subtasks[i];
                    let instance_name = format!("Veda-{}", self.instances.len() + 1);
                    
                    // Parse the fallback subtask like a real Gemma result
                    let task_parts: Vec<&str> = subtask_line.split(" | ").collect();
                    let task_desc = task_parts.get(0)
                        .unwrap_or(&"")
                        .trim_start_matches("SUBTASK_")
                        .trim_start_matches("1: ")
                        .trim_start_matches("2: ")
                        .trim_start_matches("3: ");
                    
                    let scope = task_parts.iter()
                        .find(|part| part.starts_with("SCOPE:"))
                        .map(|s| s.trim_start_matches("SCOPE:").trim())
                        .unwrap_or("No specific scope");
                        
                    let priority = task_parts.iter()
                        .find(|part| part.starts_with("PRIORITY:"))
                        .map(|s| s.trim_start_matches("PRIORITY:").trim())
                        .unwrap_or("Medium");
                    
                    let smart_task = format!("Work on: {}", task_desc);
                    
                    let coordination_message = format!(
                        r#"{}

🤝 MULTI-INSTANCE COORDINATION MODE

You are part of a coordinated team of Claude instances working on a shared codebase.

YOUR ASSIGNED TASK: {}
SCOPE: {}
PRIORITY: {}
WORKING DIRECTORY: {}

COORDINATION PROTOCOL:
1. Use TaskMaster AI tools to stay in sync:
   - mcp__taskmaster-ai__get_tasks: Check current task status
   - mcp__taskmaster-ai__set_task_status: Mark tasks done/in-progress
   - mcp__taskmaster-ai__add_task: Add discovered subtasks
   
2. Focus on your assigned work to avoid conflicts
3. Update main instance (Tab 1) with major progress
4. Use TaskMaster to communicate completion status

IMPORTANT: Work efficiently and coordinate via TaskMaster!"#,
                        Self::create_capabilities_prompt(),
                        smart_task,
                        scope,
                        priority,
                        working_dir
                    );
                    
                    // Create the ClaudeInstance first so we have a real instance ID
                    let mut new_instance = ClaudeInstance::new(instance_name.clone());
                    new_instance.working_directory = working_dir.to_string();
                    new_instance.add_message("System".to_string(), coordination_message.clone());
                    
                    let instance_id = new_instance.id; // Get the real instance ID
                    tracing::info!("Creating new tab: {} with instance_id: {}", instance_name, instance_id);
                    self.instances.push(new_instance);
                    // Session ID will be assigned when user first sends a message
                    
                    tracing::info!("Starting Claude Code instance for: {}", smart_task);
                    
                    // Auto-start the Claude Code process with the real instance ID
                    let tx = self.message_tx.clone();
                    let instance_name_owned = instance_name.clone();
                    let coordination_message_owned = coordination_message.clone();
                    let instance_id_owned = instance_id; // Use the real instance ID
                    
                    tokio::spawn(async move {
                        // Wait a moment to ensure the UI has been updated
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        
                        // Pre-enable essential tools for the spawned instance
                        let essential_tools = ["Edit", "MultiEdit", "Read", "Write", "Bash", "TodoRead", "TodoWrite", "Glob", "Grep", "LS"];
                        for tool in essential_tools.iter() {
                            if let Err(e) = enable_claude_tool(tool).await {
                                tracing::warn!("Failed to pre-enable tool {} for generic instance: {}", tool, e);
                            } else {
                                tracing::debug!("Pre-enabled tool {} for generic instance", tool);
                            }
                        }
                        
                        tracing::info!("🚀 Auto-starting Claude Code for instance {} ({})", instance_id_owned, instance_name_owned);
                        
                        // Spawn Claude Code instance with the real instance ID
                        let spawn_result = crate::claude::send_to_claude_with_session(
                            coordination_message_owned,
                            tx.clone(),
                            None, // No session ID - let Claude generate one
                            None,
                        ).await;
                        
                        tracing::info!("🏁 Claude Code spawn result for {} ({}): {:?}", instance_name_owned, instance_id_owned, spawn_result.is_ok());
                        
                        match spawn_result {
                            Ok(()) => {
                                tracing::info!("✅ Successfully started Claude Code instance for {} ({})", instance_name_owned, instance_id_owned);
                            }
                            Err(e) => {
                                tracing::error!("Failed to start Claude Code instance for {} ({}): {}", instance_name_owned, instance_id_owned, e);
                            }
                        }
                    });
                }
                
                // Don't switch tabs - let user manually switch to new tabs when they appear
            }
            return;
        }
        
        // Determine how many instances to spawn
        let instances_to_spawn = if requested_count > 0 {
            requested_count.min(self.max_instances - self.instances.len())
        } else {
            subtasks.len().min(self.max_instances - self.instances.len())
        };
        
        // Add coordination message to main instance
        if let Some(main_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
            main_instance.add_message("System".to_string(), 
                format!("🤝 Coordinating {} parallel instances for task division", instances_to_spawn));
        }
        
        // Spawn additional instances for each subtask (or up to requested count)
        for i in 0..instances_to_spawn {
            if self.instances.len() >= self.max_instances {
                break;
            }
            
            let subtask = subtasks.get(i % subtasks.len()).unwrap_or(&"General coordination task");
            
            let instance_name = format!("Veda-{}", self.instances.len() + 1);
            let mut new_instance = ClaudeInstance::new(instance_name);
            new_instance.working_directory = working_dir.to_string();
            
            // Parse subtask details
            let task_parts: Vec<&str> = subtask.split(" | ").collect();
            let task_desc = task_parts.get(0)
                .unwrap_or(&"")
                .trim_start_matches("SUBTASK_")
                .trim_start_matches("1: ")
                .trim_start_matches("2: ")
                .trim_start_matches("3: ");
            
            let scope = task_parts.iter()
                .find(|part| part.starts_with("SCOPE:"))
                .map(|s| s.trim_start_matches("SCOPE:").trim())
                .unwrap_or("No specific scope");
                
            let priority = task_parts.iter()
                .find(|part| part.starts_with("PRIORITY:"))
                .map(|s| s.trim_start_matches("PRIORITY:").trim())
                .unwrap_or("Medium");
            
            // Send coordination context to new instance
            let coordination_message = format!(
                r#"{}

🤝 MULTI-INSTANCE COORDINATION MODE

You are part of a coordinated team of Claude instances working on a shared codebase.

YOUR ASSIGNED SUBTASK: {}
SCOPE: {}
PRIORITY: {}
WORKING DIRECTORY: {}

COORDINATION PROTOCOL:
1. Use TaskMaster AI tools to stay in sync:
   - mcp__taskmaster-ai__get_tasks: Check current task status
   - mcp__taskmaster-ai__set_task_status: Mark tasks done/in-progress
   - mcp__taskmaster-ai__add_task: Add discovered subtasks
   
2. Focus ONLY on your assigned scope to avoid conflicts
3. Update main instance (Tab 1) with major progress
4. Use TaskMaster to communicate completion status

IMPORTANT: Work within your scope and coordinate via TaskMaster!"#,
                Self::create_capabilities_prompt(),
                task_desc,
                scope,
                priority,
                working_dir
            );
            
            new_instance.add_message("System".to_string(), coordination_message);
            
            let instance_id = new_instance.id;
            let instance_name_copy = new_instance.name.clone();
            self.instances.push(new_instance);
            // Session ID will be assigned when user first sends a message
            
            // Switch to the new instance briefly to show it was created
            if i == 0 {
                self.current_tab = self.instances.len() - 1;
            }
            
            tracing::info!("Spawned coordinated instance {} for subtask: {}", instance_id, task_desc);
            
            // Auto-start the instance with its task in the background
            let tx = self.message_tx.clone();
            let task_instruction = format!(
                "Please begin working on your assigned subtask: {}\n\nScope: {}\nPriority: {}\n\nStart by using mcp__taskmaster-ai__get_tasks to check the current project status, then focus on your specific scope.",
                task_desc, scope, priority
            );
            
            let instance_id_copy = instance_id;
            let instance_name_copy2 = instance_name_copy.clone();
            // Capture coordinator's session ID for status messages
            let coordinator_session_id = self.instances[0].session_id.clone();
            
            // Start the Claude process for this instance automatically
            tokio::spawn(async move {
                // Wait a moment to ensure the UI has been updated
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                
                // Pre-enable essential tools for the spawned instance
                let essential_tools = ["Edit", "MultiEdit", "Read", "Write", "Bash", "TodoRead", "TodoWrite", "Glob", "Grep", "LS"];
                for tool in essential_tools.iter() {
                    if let Err(e) = enable_claude_tool(tool).await {
                        tracing::warn!("Failed to pre-enable tool {} for spawned instance: {}", tool, e);
                    } else {
                        tracing::debug!("Pre-enabled tool {} for spawned instance", tool);
                    }
                }
                
                tracing::info!("Auto-starting Claude Code instance {} ({}) with task", instance_name_copy2, instance_id_copy);
                
                // Spawn Claude Code instance with the task instruction
                // First set the instance-specific environment variable
                std::env::set_var("VEDA_TARGET_INSTANCE_ID", instance_id_copy.to_string());
                
                let spawn_result = crate::claude::send_to_claude_with_session(
                    task_instruction.clone(),
                    tx.clone(),
                    None, // No existing session for new instance
                    None, // No process handle storage needed for new instance
                ).await;
                
                // Clean up the environment variable
                std::env::remove_var("VEDA_TARGET_INSTANCE_ID");
                
                match spawn_result {
                    Ok(()) => {
                        tracing::info!("✅ Successfully started Claude Code instance for {}", instance_name_copy2);
                        
                        // Send success message to coordinator instance  
                        let _ = tx.send(ClaudeMessage::StreamText {
                            text: format!("✅ Started Claude Code instance {} with task", instance_name_copy2),
                            session_id: coordinator_session_id.clone(),
                        }).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to start Claude Code instance for {}: {}", instance_name_copy2, e);
                        // Send error message to coordinator instance
                        let _ = tx.send(ClaudeMessage::StreamText {
                            text: format!("❌ Failed to start Claude Code instance: {}", e),
                            session_id: coordinator_session_id.clone(),
                        }).await;
                    }
                }
            });
        }
        
        // Collect instance names first to avoid borrowing issues
        let instance_names: Vec<String> = self.instances.iter()
            .skip(1) // Skip Tab 1
            .map(|inst| inst.name.clone())
            .collect();
        
        // Assign work to the main instance and provide coordination details
        if let Some(main_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
            // Determine main instance's work assignment
            let main_task = if !subtasks.is_empty() {
                // Assign the first/highest priority subtask to main instance
                let first_subtask = subtasks[0];
                let task_parts: Vec<&str> = first_subtask.split(" | ").collect();
                let task_desc = task_parts.get(0)
                    .unwrap_or(&"")
                    .trim_start_matches("SUBTASK_")
                    .trim_start_matches("1: ");
                let scope = task_parts.iter()
                    .find(|part| part.starts_with("SCOPE:"))
                    .map(|s| s.trim_start_matches("SCOPE:").trim())
                    .unwrap_or("Project coordination");
                format!("YOUR ASSIGNED TASK: {}\nSCOPE: {}", task_desc, scope)
            } else {
                "YOUR ASSIGNED TASK: Project coordination and high-level development\nSCOPE: Overall project architecture and integration".to_string()
            };

            main_instance.add_message("System".to_string(), 
                format!("✅ Spawned {} coordinated instances: {}\n\n🎯 MAIN INSTANCE COORDINATION ASSIGNMENT:\n{}\n\nCOORDINATION RESPONSIBILITIES:\n- Lead the overall project development\n- Use mcp__taskmaster-ai__get_tasks to monitor all instances\n- Integrate work from spawned instances\n- Switch tabs (Ctrl+Left/Right) to monitor progress\n- Each instance will update TaskMaster as they complete work\n\n⚡ BEGIN WORKING: Start with your assigned task immediately!", 
                instances_to_spawn, 
                instance_names.join(", "),
                main_task
            ));
        }

        // Only auto-start the main instance if it doesn't already have a session
        // This prevents creating a new Claude process that would overwrite the existing session
        if let Some(main_instance) = self.instances.iter().find(|i| i.id == main_instance_id) {
            if main_instance.session_id.is_none() {
                tracing::info!("Main instance has no session, will auto-start with coordination task");
                
                let main_task_instruction = if !subtasks.is_empty() {
                    let first_subtask = subtasks[0];
                    let task_parts: Vec<&str> = first_subtask.split(" | ").collect();
                    let task_desc = task_parts.get(0)
                        .unwrap_or(&"")
                        .trim_start_matches("SUBTASK_")
                        .trim_start_matches("1: ");
                    let scope = task_parts.iter()
                        .find(|part| part.starts_with("SCOPE:"))
                        .map(|s| s.trim_start_matches("SCOPE:").trim())
                        .unwrap_or("Project coordination");
                    format!("Please begin working on your assigned task: {}\n\nScope: {}\n\nAs the main coordination instance, start by:\n1. Using mcp__taskmaster-ai__get_tasks to check project status\n2. Beginning work on your specific scope\n3. Coordinating with other instances as needed\n\nStart working immediately!", task_desc, scope)
                } else {
                    "Please begin coordinating the project development. Start by:\n1. Using mcp__taskmaster-ai__get_tasks to check project status\n2. Providing high-level guidance and architecture decisions\n3. Monitoring progress from spawned instances\n\nStart working immediately!".to_string()
                };

                let tx = self.message_tx.clone();
                let main_session_id = main_instance.session_id.clone();
                
                tokio::spawn(async move {
                    // Wait a moment for the spawning messages to complete
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    
                    tracing::info!("Auto-starting main instance with coordination task");
                    if let Err(e) = send_to_claude_with_session(main_task_instruction, tx, main_session_id, None).await {
                        tracing::error!("Failed to auto-start main instance: {}", e);
                    }
                });
            } else {
                tracing::info!("Main instance already has session {:?}, skipping auto-start to preserve existing Claude process", main_instance.session_id);
            }
        }
        
        // Switch back to main instance (Tab 1) so user can see it starting to work
        // This shows that the main instance has been assigned work and is active
        self.current_tab = 0;
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

async fn start_ipc_server(app_tx: mpsc::Sender<ClaudeMessage>, session_id: String) {
    use tokio::net::{UnixListener, UnixStream};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use crate::shared_ipc::RegistryClient;
    
    // Create a shared socket path that all Veda instances can use
    let socket_path = "/tmp/veda-shared.sock";
    
    // Try to bind the socket, if it fails, another instance is already running
    match UnixListener::bind(&socket_path) {
        Ok(listener) => {
            tracing::info!("Started shared IPC server on {}", socket_path);
            
            // This is the first instance, run the shared server
            loop {
                match listener.accept().await {
                    Ok((socket, _)) => {
                        let app_tx = app_tx.clone();
                        tokio::spawn(handle_shared_ipc_connection(socket, app_tx));
                    }
                    Err(e) => {
                        tracing::error!("Failed to accept IPC connection: {}", e);
                    }
                }
            }
        }
        Err(_) => {
            // Another instance is already running the server
            tracing::info!("Shared IPC server already running, using client mode");
        }
    }
}

async fn handle_shared_ipc_connection(mut socket: tokio::net::UnixStream, app_tx: mpsc::Sender<ClaudeMessage>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    
    let (reader, mut writer) = socket.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    
    while reader.read_line(&mut line).await.is_ok() {
        if line.is_empty() {
            break;
        }
        
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
            tracing::info!("Shared IPC received message: {:?}", msg["type"]);
            
            let response = match msg["type"].as_str() {
                Some("spawn_instances") => {
                    let task_desc = msg["task_description"].as_str().unwrap_or("");
                    let num_instances = msg["num_instances"].as_u64().unwrap_or(2) as u8;
                    let session_id = msg["session_id"].as_str().unwrap_or("");
                    
                    // Get target instance ID from the IPC message if provided
                    let instance_id = if let Some(target_id_str) = msg["target_instance_id"].as_str() {
                        match Uuid::parse_str(target_id_str) {
                            Ok(id) => id,
                            Err(e) => {
                                tracing::warn!("Failed to parse target_instance_id '{}': {}", target_id_str, e);
                                Uuid::new_v4()
                            }
                        }
                    } else {
                        Uuid::new_v4()
                    };
                    
                    // Update shared registry
                    if let Err(e) = crate::shared_ipc::RegistryClient::increment_instances(session_id, num_instances as u32).await {
                        tracing::error!("Failed to update registry: {}", e);
                    }
                    
                    // Send message to appropriate Veda instance
                    let _ = app_tx.send(ClaudeMessage::VedaSpawnInstances {
                        instance_id,
                        task_description: task_desc.to_string(),
                        num_instances,
                    }).await;
                    
                    format!("✅ Spawning {} instances for task: {}", num_instances, task_desc)
                }
                Some("list_instances") => {
                    let session_id = msg["session_id"].as_str().unwrap_or("");
                    
                    // Query shared registry
                    match crate::shared_ipc::RegistryClient::get_instances(session_id).await {
                        Ok(count) => format!("✅ Session {} has {} child instances", session_id, count),
                        Err(e) => format!("❌ Failed to query registry: {}", e),
                    }
                }
                Some("close_instance") => {
                    let instance_name = msg["instance_name"].as_str().unwrap_or("");
                    let session_id = msg["session_id"].as_str().unwrap_or("");
                    
                    let instance_id = if let Some(target_id_str) = msg["target_instance_id"].as_str() {
                        match Uuid::parse_str(target_id_str) {
                            Ok(id) => id,
                            Err(_) => Uuid::new_v4()
                        }
                    } else {
                        Uuid::new_v4()
                    };
                    
                    // Update shared registry
                    if let Err(e) = crate::shared_ipc::RegistryClient::decrement_instances(session_id, 1).await {
                        tracing::error!("Failed to update registry: {}", e);
                    }
                    
                    let _ = app_tx.send(ClaudeMessage::VedaCloseInstance {
                        instance_id,
                        target_instance_name: instance_name.to_string(),
                    }).await;
                    
                    format!("✅ Closing instance: {}", instance_name)
                }
                Some("registry_status") => {
                    // List all active sessions
                    match crate::shared_ipc::RegistryClient::list_all_sessions().await {
                        Ok(sessions) => {
                            let status_lines: Vec<String> = sessions.iter()
                                .map(|(sid, count)| format!("  Session {}: {} instances", sid, count))
                                .collect();
                            format!("📊 Registry Status:\n{}", status_lines.join("\n"))
                        }
                        Err(e) => format!("❌ Failed to query registry: {}", e),
                    }
                }
                Some("coordination_message") => {
                    // Inter-Veda coordination message
                    if let Ok(coord_msg) = serde_json::from_value::<crate::shared_ipc::VedaCoordinationMessage>(msg.clone()) {
                        tracing::info!("Received coordination message from {} to {:?}: {}", 
                            coord_msg.from, coord_msg.to, coord_msg.summary);
                        
                        // For now, route to the main app for processing
                        // In the future, this could be smarter routing based on repository context
                        let _ = app_tx.send(crate::claude::ClaudeMessage::CoordinationMessage {
                            message: coord_msg,
                        }).await;
                        
                        "✅ Coordination message routed".to_string()
                    } else {
                        "❌ Invalid coordination message format".to_string()
                    }
                }
                _ => "❌ Unknown command".to_string(),
            };
            
            let _ = writer.write_all(response.as_bytes()).await;
            let _ = writer.write_all(b"\n").await;
        }
        
        line.clear();
    }
}

// Standalone function for background Gemma analysis
async fn perform_gemma_analysis(prompt: &str) -> Result<String> {
    // Try with optimized prompt for faster response
    let optimized_prompt = format!(
        "{}\n\nIMPORTANT: Respond ONLY in the requested format. Skip chain-of-thought. Be direct.",
        prompt
    );
    
    let request_body = serde_json::json!({
        "model": "gemma3:12b",
        "prompt": optimized_prompt,
        "stream": false,
        "options": {
            "temperature": 0.1,
            "top_p": 0.9,
            "num_predict": 500
        }
    });
    
    let client = reqwest::Client::new();
    
    // Retry up to 3 times with shorter timeout per request
    let mut retry_count = 0;
    let max_retries = 3;
    
    loop {
        match client
            .post("http://localhost:11434/api/generate")
            .json(&request_body)
            .timeout(Duration::from_secs(30))
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    #[derive(serde::Deserialize)]
                    struct OllamaResponse {
                        response: String,
                    }
                    
                    match response.json::<OllamaResponse>().await {
                        Ok(ollama_response) => {
                            return Ok(ollama_response.response.trim().to_string());
                        }
                        Err(e) => {
                            tracing::error!("Failed to parse Ollama response: {}", e);
                            return Err(anyhow::anyhow!("Failed to parse Ollama response: {}", e));
                        }
                    }
                } else {
                    tracing::warn!("Ollama API error: status {}", response.status());
                    if retry_count >= max_retries {
                        return Err(anyhow::anyhow!("Ollama API error after {} retries", max_retries));
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to contact Ollama (attempt {}/{}): {}", retry_count + 1, max_retries, e);
                if retry_count >= max_retries {
                    return Err(anyhow::anyhow!("Failed to contact Ollama after {} retries: {}", max_retries, e));
                }
            }
        }
        
        // Exponential backoff: 1s, 2s, 4s, 8s, etc. (capped at 60s)
        retry_count += 1;
        let delay_secs = std::cmp::min(1u64 << (retry_count - 1), 60);
        tracing::info!("Retrying Ollama request in {} seconds...", delay_secs);
        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging to debug.log in current working directory
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let log_file_path = cwd.join("debug.log");
    
    // Open the log file in append mode (create if doesn't exist)
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&log_file_path)
        .unwrap_or_else(|e| {
            eprintln!("Failed to open debug.log: {}", e);
            // Fallback to creating a new file
            std::fs::File::create(&log_file_path).expect("Failed to create debug.log")
        });
    
    // Use the file directly instead of rolling appender to ensure append mode
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter("veda_tui=debug")
        .init();
    
    tracing::info!("Starting Veda TUI from directory: {:?}", cwd);
    tracing::info!("Debug log path: {:?}", log_file_path);
    
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new()?;
    
    // Check for instance name from environment (for spawned instances)
    if let Ok(instance_name) = std::env::var("VEDA_INSTANCE_NAME") {
        // This is a spawned instance - update the main instance name
        if !app.instances.is_empty() {
            app.instances[0].name = instance_name;
            tracing::info!("Updated instance name from environment: {}", app.instances[0].name);
        }
    }
    
    // Check for auto-start task from environment
    if let Ok(auto_task) = std::env::var("VEDA_AUTO_TASK") {
        tracing::info!("Auto-task detected from environment: {}", auto_task);
        // Add the auto-task as an initial message to process
        if !app.instances.is_empty() {
            app.instances[0].add_message("System".to_string(), format!("Auto-starting with task: {}", auto_task));
            
            // Store auto-task to send once instance 0 has a session ID
            app.pending_auto_task = Some(auto_task);
        }
    }
    
    // Set the session ID as environment variable for child processes
    std::env::set_var("VEDA_SESSION_ID", &app.session_id);
    
    // Get session ID and start IPC server
    let session_id = app.session_id.clone();
    let ipc_tx = app.message_tx.clone();
    
    // Start the IPC server in the background
    tokio::spawn(async move {
        start_ipc_server(ipc_tx, session_id).await;
    });
    
    tracing::info!("Started Veda with session ID: {}", app.session_id);

    // Run the UI - keep _guard alive by moving it into the async block
    let res = run_app(&mut terminal, &mut app, _guard).await;

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
    
    // Clean up Unix socket
    let socket_path = format!("/tmp/veda-{}.sock", app.session_id);
    let _ = std::fs::remove_file(&socket_path);
    tracing::info!("Cleaned up socket: {}", socket_path);

    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App, _guard: tracing_appender::non_blocking::WorkerGuard) -> Result<()> {
    'outer: loop {
        // Process any Claude messages
        app.process_claude_messages().await;
        
        // Process any DeepSeek messages
        app.process_deepseek_messages().await;
        
        // Check for stalled conversations
        app.check_for_stalls().await;
        
        // Debug check for empty tabs bug
        let total_messages: usize = app.instances.iter().map(|i| i.messages.len()).sum();
        if total_messages > 0 && app.instances.iter().all(|i| {
            // Check if the instance appears to have no visible content
            i.messages.is_empty() || i.scroll_offset > i.messages.len() as u16 * 2
        }) {
            tracing::error!("UI BUG DETECTED: {} total messages but all tabs appear empty!", total_messages);
            for (idx, instance) in app.instances.iter().enumerate() {
                tracing::error!("Tab {}: {} messages, scroll_offset={}, last_height={}", 
                    idx, instance.messages.len(), instance.scroll_offset, instance.last_message_area_height);
            }
            // Force reset scroll offsets as recovery
            for instance in app.instances.iter_mut() {
                instance.scroll_offset = 0;
            }
        }
        
        // Check if todo list should be hidden
        if app.should_hide_todo_list() {
            app.hide_todo_list();
        }
        
        // Add error recovery for terminal drawing with timeout protection
        let draw_start = std::time::Instant::now();
        if let Err(e) = terminal.draw(|f| ui(f, app)) {
            tracing::error!("Terminal draw error: {:?}", e);
            // Try to recover by hiding cursor and clearing
            let _ = terminal.hide_cursor();
            let _ = terminal.clear();
            // Force a redraw on next iteration
            continue;
        }
        let draw_duration = draw_start.elapsed();
        if draw_duration.as_millis() > 500 {
            tracing::warn!("Slow UI render detected: {:?}ms", draw_duration.as_millis());
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Paste(data) => {
                    // Handle paste event - insert text directly into textarea
                    if let Some(instance) = app.current_instance_mut() {
                        // Track user input for stall detection
                        instance.on_user_input();
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
                Event::Key(key) => {
                    // DO NOT LOG KEYSTROKES - SECURITY RISK
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
                        (KeyModifiers::CONTROL, KeyCode::Char('x')) => app.close_current_instance(),
                        (KeyModifiers::CONTROL, KeyCode::Char('a')) => app.toggle_auto_mode(),
                        (KeyModifiers::CONTROL, KeyCode::Char('t')) => app.toggle_chain_of_thought(),
                        (KeyModifiers::CONTROL, KeyCode::Char('m')) => app.toggle_coordination_mode(),
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
                                tracing::debug!("Shift+Enter pressed, adding new line");
                                // Manually insert a new line
                                instance.textarea.insert_newline();
                            }
                        }
                        (_, KeyCode::Enter) => {
                            let now = std::time::Instant::now();
                            
                            // Handle triple-Enter interruption detection
                            let should_interrupt = if let Some(last_time) = app.last_enter_time {
                                if now.duration_since(last_time).as_millis() < 500 { // Within 500ms
                                    app.enter_press_count += 1;
                                    if app.enter_press_count >= 3 {
                                        app.enter_press_count = 0;
                                        true // Trigger interruption
                                    } else {
                                        false
                                    }
                                } else {
                                    app.enter_press_count = 1;
                                    false
                                }
                            } else {
                                app.enter_press_count = 1;
                                false
                            };
                            app.last_enter_time = Some(now);
                            
                            if should_interrupt {
                                // Triple-Enter: Interrupt current instance and process queue
                                let (should_interrupt, current_message) = {
                                    if let Some(instance) = app.current_instance_mut() {
                                        if instance.is_processing {
                                            tracing::info!("Triple-Enter detected: interrupting instance {}", instance.id);
                                            // Extract current input if not empty
                                            let current_message = if !instance.textarea.is_empty() {
                                                let msg = instance.textarea.lines().join("\n");
                                                instance.textarea = TextArea::default();
                                                instance.textarea.set_block(
                                                    Block::default()
                                                        .borders(Borders::ALL)
                                                        .title("Input")
                                                );
                                                Some(msg)
                                            } else {
                                                None
                                            };
                                            (true, current_message)
                                        } else {
                                            (false, None)
                                        }
                                    } else {
                                        (false, None)
                                    }
                                };
                                
                                if should_interrupt {
                                    // Add current input to queue if exists
                                    if let Some(msg) = current_message {
                                        app.message_queue.push(msg);
                                    }
                                    // Send SIGINT to interrupt the process
                                    app.interrupt_current_instance().await;
                                }
                            } else {
                                // Regular Enter: Add to queue or send immediately
                                let (message, is_processing) = {
                                    if let Some(instance) = app.current_instance_mut() {
                                        if !instance.textarea.is_empty() {
                                            let message = instance.textarea.lines().join("\n");
                                            let is_processing = instance.is_processing;
                                            instance.textarea = TextArea::default();
                                            instance.textarea.set_block(
                                                Block::default()
                                                    .borders(Borders::ALL)
                                                    .title("Input")
                                            );
                                            (Some(message), is_processing)
                                        } else {
                                            (None, false)
                                        }
                                    } else {
                                        (None, false)
                                    }
                                };
                                
                                if let Some(message) = message {
                                    if is_processing {
                                        // Instance is busy, add to queue
                                        let queue_len = app.message_queue.len() + 1;
                                        app.message_queue.push(message);
                                        if let Some(instance) = app.current_instance_mut() {
                                            instance.add_message("System".to_string(), 
                                                format!("📬 Message queued ({} in queue)", queue_len));
                                        }
                                    } else {
                                        // Instance is free, send immediately
                                        app.send_message(message).await;
                                    }
                                }
                            }
                        }
                        _ => {
                            // Pass all other key events to the textarea
                            if let Some(instance) = app.current_instance_mut() {
                                // Track user input for stall detection
                                instance.on_user_input();
                                use ratatui::crossterm::event::Event as RatatuiEvent;
                                instance.textarea.input(RatatuiEvent::Key(key));
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::Down(_) => {
                            // Check if click is on any tab using calculated rectangles
                            for (i, tab_rect) in app.tab_rects.iter().enumerate() {
                                if mouse.column >= tab_rect.x 
                                    && mouse.column < tab_rect.x + tab_rect.width
                                    && mouse.row == tab_rect.y {
                                    app.current_tab = i;
                                    app.sync_working_directory();
                                    
                                    // Log tab switch with session info
                                    if let Some(instance) = app.instances.get(i) {
                                        tracing::info!("Clicked tab {} ({}) at ({}, {}) - Session: {:?}", 
                                            i, instance.name, mouse.column, mouse.row, instance.session_id);
                                    } else {
                                        tracing::info!("Clicked tab {} at ({}, {}) - No instance found", i, mouse.column, mouse.row);
                                    }
                                    continue 'outer;
                                }
                            }
                            
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
    // Update terminal width
    app.terminal_width = f.area().width;
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
            Constraint::Length(1),
        ])
        .split(f.area());

    // Header with tabs
    let titles: Vec<Line> = app
        .instances
        .iter()
        .map(|instance| Line::from(instance.name.clone()))
        .collect();
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Veda Claude Manager "))
        .select(app.current_tab)
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[0]);
    
    // Calculate tab rectangles for click detection
    app.tab_rects.clear();
    if !app.instances.is_empty() {
        let tab_area = Rect {
            x: chunks[0].x + 1, // Inside border
            y: chunks[0].y + 1, // Inside border
            width: chunks[0].width - 2, // Minus borders
            height: 1, // Tab height
        };
        
        let mut current_x = tab_area.x;
        for instance in &app.instances {
            let tab_width = instance.name.len() as u16 + 2; // +2 for padding like " Claude 1 "
            let tab_rect = Rect {
                x: current_x,
                y: tab_area.y,
                width: tab_width,
                height: 1,
            };
            app.tab_rects.push(tab_rect);
            current_x += tab_width;
        }
    }

    // Messages area
    // First, update dimensions for ALL instances so background tabs work correctly
    let message_area_height = chunks[1].height;
    let message_area_width = chunks[1].width.saturating_sub(2); // Subtract borders
    
    for instance in app.instances.iter_mut() {
        // Store dimensions for ALL tabs, not just current one
        instance.last_message_area_height = message_area_height;
        instance.last_terminal_width = message_area_width;
    }
    
    if let Some(instance) = app.instances.get_mut(app.current_tab) {
        // Update scroll position for the current tab only
        instance.auto_scroll_with_width(Some(message_area_height), Some(message_area_width));
        
        // Debug check for empty display bug
        if !instance.messages.is_empty() {
            tracing::debug!("Rendering tab {} with {} messages", app.current_tab, instance.messages.len());
            if instance.messages.len() > 5 {
                // Log a sample of messages to verify they exist
                tracing::debug!("Sample messages: first={:?}, last={:?}", 
                    instance.messages.first().map(|m| format!("{}: {}", m.sender, m.content.chars().take(50).collect::<String>())),
                    instance.messages.last().map(|m| format!("{}: {}", m.sender, m.content.chars().take(50).collect::<String>()))
                );
            }
        }
        
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
                        "Error" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        "DeepSeekError" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
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
                let line_y = i as u16;
                let start_y = start.1.min(end.1);
                let end_y = start.1.max(end.1);
                
                if line_y >= start_y && line_y <= end_y {
                    style = style.add_modifier(Modifier::REVERSED);
                }
            }
            
            // Safely create line with error recovery
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Line::from(content).style(style)
            })) {
                Ok(line) => {
                    all_lines.push(line);
                    // Add empty line between messages for readability
                    all_lines.push(Line::from(""));
                }
                Err(e) => {
                    tracing::error!("Failed to render message line {}: {:?}", i, e);
                    // Add placeholder line to prevent UI corruption
                    all_lines.push(Line::from(format!("[Error rendering message {}]", i)));
                    all_lines.push(Line::from(""));
                }
            }
        }

        let current_dir = if let Ok(home) = std::env::var("HOME") {
            if instance.working_directory.starts_with(&home) {
                instance.working_directory.replacen(&home, "~", 1)
            } else {
                instance.working_directory.clone()
            }
        } else {
            instance.working_directory.clone()
        };
        // Debug check before rendering
        if all_lines.is_empty() && !instance.messages.is_empty() {
            tracing::error!("CRITICAL UI BUG: {} messages but no lines generated for rendering!", instance.messages.len());
            tracing::error!("First message: {:?}", instance.messages.first());
            // Add fallback content
            all_lines.push(Line::from("[Error: Failed to render messages]"));
        }
        
        let messages_paragraph = Paragraph::new(all_lines)
            .block(Block::default().borders(Borders::ALL).title(format!(
                "Messages - {} [Auto: {}] [CoT: {}] [Coord: {}] [Dir: {}]{}",
                instance.name,
                if app.auto_mode { "ON" } else { "OFF" },
                if app.show_chain_of_thought { "ON" } else { "OFF" },
                if app.coordination_enabled { "ON" } else { "OFF" },
                current_dir,
                if let Some(ref sid) = instance.session_id {
                    let display_len = 8.min(sid.len());
                    let start = sid.len().saturating_sub(display_len);
                    format!(" [Session: ...{}]", &sid[start..])
                } else {
                    String::new()
                }
            )))
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false })
            .scroll((instance.scroll_offset, 0));
        f.render_widget(messages_paragraph, chunks[1]);

        // Input area with tui-textarea
        let title = if instance.is_processing {
            if app.message_queue.is_empty() {
                "Input (Processing...)".to_string()
            } else {
                format!("Input (Processing... {} queued)", app.message_queue.len())
            }
        } else if !app.message_queue.is_empty() {
            format!("Input ({} queued - Enter to send)", app.message_queue.len())
        } else {
            "Input (Enter to send, Shift+Enter for new line, 3x Enter to interrupt)".to_string()
        };
        
        instance.textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
        );
        f.render_widget(&instance.textarea, chunks[2]);
    }
    
    // Status bar with hotkeys
    let status_line = "Ctrl+N: New Tab | Ctrl+X: Close Tab | Ctrl+L/R: Switch | Ctrl+A: Auto | Ctrl+T: CoT | Ctrl+M: Coord | Ctrl+D: Todo | Ctrl+C: Copy/Exit | !cd: ChangeDir ";
    let status_bar = Paragraph::new(status_line)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .alignment(Alignment::Left);
    f.render_widget(status_bar, chunks[3]);
    
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
        Span::styled("TodoTasks", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from("")); // Empty line
    
    if todo_list.items.is_empty() {
        lines.push(Line::from(Span::styled(
            "No tasks ", 
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
        )));
    } else {
        for item in &todo_list.items {
            let status_emoji = match item.status.as_str() {
                "done" => "✅",
                "in_progress" => "🔄",
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
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left);
    
    f.render_widget(todo_widget, popup_area);
}

