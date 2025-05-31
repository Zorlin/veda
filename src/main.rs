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
    // Claude session ID for resume
    session_id: Option<String>,
    // Working directory for this tab
    working_directory: String,
    // Stall detection
    last_activity: DateTime<Local>,
    stall_check_sent: bool,
    stall_delay_seconds: i64, // Dynamic delay: 10s, 20s, 30s max
    stall_intervention_in_progress: bool, // Prevent multiple simultaneous interventions
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
            working_directory: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            last_activity: Local::now(),
            stall_check_sent: false,
            stall_delay_seconds: 10, // Start with 10 second delay
            stall_intervention_in_progress: false,
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
        
        // Auto-scroll to show new messages (will be updated with actual height in UI)
        self.auto_scroll_to_bottom(None);
    }
    
    fn auto_scroll_to_bottom(&mut self, message_area_height: Option<u16>) {
        // Calculate how many lines are needed for all messages
        // Account for message content that might wrap and collapsed thinking sections
        let mut total_lines = 0;
        
        for msg in &self.messages {
            // Each message has timestamp + sender + content
            if msg.sender == "DeepSeek" && msg.is_thinking && msg.is_collapsed {
                // Collapsed thinking shows as one line
                total_lines += 1;
            } else {
                // Normal message - estimate wrapped lines based on content length
                let content_lines = (msg.content.len() / 80).max(1); // Rough estimate for 80-char width
                total_lines += content_lines + 1; // +1 for empty line between messages
            }
        }
        
        // Use provided height or fallback to default
        let visible_lines = message_area_height.unwrap_or(20) as usize;
        
        if total_lines > visible_lines {
            self.scroll_offset = (total_lines - visible_lines) as u16;
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
            "model": "deepseek-r1:8b",
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
            terminal_width: 80, // Default, will be updated in draw
            tab_rects: Vec::new(),
            coordination_enabled: true,
            max_instances: 4, // Main + 3 additional
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
        
        // Collect necessary data first to avoid borrowing conflicts
        let (id, session_id, working_dir, is_first_message) = {
            if let Some(instance) = self.current_instance_mut() {
                tracing::info!("Sending message to Claude instance {}: {}", instance.id, message);
                instance.add_message("You".to_string(), message.clone());
                instance.is_processing = true;
                
                let id = instance.id;
                let session_id = instance.session_id.clone();
                let working_dir = instance.working_directory.clone();
                let is_first_message = instance.messages.len() == 1; // Only user message so far
                
                (id, session_id, working_dir, is_first_message)
            } else {
                return;
            }
        };
        
        let tx = self.message_tx.clone();
        
        // Create the message to send
        let mut context_message = format!("Working directory: {}\n\n", working_dir);
        
        // Add capabilities prompt for first message in a session
        if is_first_message {
            context_message.push_str(&Self::create_capabilities_prompt());
            context_message.push_str("\n\n---\n\n");
        }
        
        context_message.push_str(&message);
        
        // Send to Claude
        tokio::spawn(async move {
            tracing::debug!("Spawning send_to_claude task for instance {} with session {:?} in dir {}", id, session_id, working_dir);
            if let Err(e) = send_to_claude_with_session(id, context_message, tx, session_id).await {
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
                                let tx = self.message_tx.clone();
                                
                                tokio::spawn(async move {
                                    tracing::info!("Sending DeepSeek verdict to Claude: {}", message_to_claude);
                                    // Send directly without working directory context since this is automode
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
                    // First, collect necessary data to avoid borrow conflicts
                    let (claude_message_opt, main_instance_id, user_context_opt) = {
                        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                            instance.is_processing = false;
                            
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
                        
                        // Check if this task would benefit from coordination
                        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                            instance.add_message("System".to_string(), 
                                "🤖 Analyzing if task would benefit from multi-instance coordination...".to_string());
                        }
                        
                        if self.analyze_task_for_coordination(&claude_message).await {
                            tracing::info!("Task identified for multi-instance coordination");
                            self.coordinate_multi_instance_task(main_instance_id, &claude_message).await;
                            return; // Don't continue with normal automode processing
                        }
                        
                        // Continue with normal automode processing - collect more data
                        let (had_tool_attempts, attempted_tools, session_id_opt) = {
                            if let Some(instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                                let had_tool_attempts = !instance.last_tool_attempts.is_empty();
                                let attempted_tools = instance.last_tool_attempts.clone();
                                
                                // Clear tool attempts for next message
                                instance.last_tool_attempts.clear();
                                
                                // Add system message if tools were attempted
                                if had_tool_attempts {
                                    instance.add_message("System".to_string(), 
                                        format!("🤖 Automode: Checking if Claude needs permission for tools: {}", attempted_tools.join(", ")));
                                }
                                
                                (had_tool_attempts, attempted_tools, instance.session_id.clone())
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
                                                        instance_id: main_instance_id,
                                                        text: system_msg,
                                                    }).await;
                                                    
                                                    // Send a message telling Claude the tools are now enabled
                                                    let response = format!(
                                                        "I've enabled the following tools for you: {}. Please try using them again.",
                                                        enabled_tools.join(", ")
                                                    );
                                                    
                                                    if let Err(e) = send_to_claude_with_session(main_instance_id, response, tx, Some(session_id)).await {
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
                                            if let Err(e) = send_to_claude_with_session(main_instance_id, coordination_response.to_string(), tx.clone(), Some(session_id.clone())).await {
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
                ClaudeMessage::ToolPermissionDenied { instance_id, tool_name } => {
                    tracing::info!("Tool permission denied for instance {}: {}", instance_id, tool_name);
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        instance.add_message("System".to_string(), format!("🔒 Permission denied for tool: {}", tool_name));
                        
                        // In automode, ask DeepSeek to analyze if this tool should be enabled
                        if self.auto_mode {
                            let tool_name_copy = tool_name.clone();
                            let instance_id_copy = instance.id;
                            let session_id = instance.session_id.clone();
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
                                                instance_id: instance_id_copy,
                                                text: format!("❌ Failed to enable {}: {}", tool_name_copy, e),
                                            }).await;
                                        } else {
                                            tracing::info!("Successfully enabled tool: {}", tool_name_copy);
                                            let _ = tx.send(ClaudeMessage::StreamText {
                                                instance_id: instance_id_copy,
                                                text: format!("🔧 Automode: Safely enabled tool: {}", tool_name_copy),
                                            }).await;
                                            
                                            // Tell Claude to try again
                                            let response = format!("I've enabled the {} tool for you. Please try using it again.", tool_name_copy);
                                            if let Err(e) = send_to_claude_with_session(instance_id_copy, response, tx, session_id).await {
                                                tracing::error!("Failed to send tool enablement message to Claude: {}", e);
                                            }
                                        }
                                    }
                                    Ok(false) => {
                                        tracing::warn!("DeepSeek determined tool {} is unsafe to enable", tool_name_copy);
                                        let _ = tx.send(ClaudeMessage::StreamText {
                                            instance_id: instance_id_copy,
                                            text: format!("🚫 Automode: Tool {} was deemed unsafe and not enabled", tool_name_copy),
                                        }).await;
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to analyze tool safety: {}", e);
                                        let _ = tx.send(ClaudeMessage::StreamText {
                                            instance_id: instance_id_copy,
                                            text: format!("⚠️ Could not analyze safety of tool {}: {}", tool_name_copy, e),
                                        }).await;
                                    }
                                }
                            });
                        }
                    }
                }
                ClaudeMessage::VedaSpawnInstances { instance_id, task_description, num_instances } => {
                    tracing::info!("Claude requested to spawn {} instances for task: {}", num_instances, task_description);
                    
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        instance.add_message("Tool".to_string(), 
                            format!("🤝 Spawning {} additional instances for task: {}", num_instances, task_description));
                    }
                    
                    // Use the existing coordination system but with Claude's specific request
                    self.coordinate_multi_instance_task_with_count(instance_id, &task_description, num_instances as usize).await;
                }
                ClaudeMessage::VedaListInstances { instance_id } => {
                    tracing::info!("Claude requested instance list");
                    
                    // Collect instance information first
                    let mut instance_info = Vec::new();
                    instance_info.push("📋 Current Claude Instances:".to_string());
                    
                    for (i, inst) in self.instances.iter().enumerate() {
                        let status = if inst.is_processing { "(Processing)" } else { "(Idle)" };
                        let current_marker = if i == self.current_tab { " ← Current" } else { "" };
                        instance_info.push(format!("  {}. {} {} - Dir: {}{}", 
                            i + 1, inst.name, status, inst.working_directory, current_marker));
                    }
                    
                    // Then send the message
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        instance.add_message("Tool".to_string(), instance_info.join("\n"));
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
            }
        }
    }

    async fn check_for_stalls(&mut self) {
        if !self.auto_mode {
            return; // Only check for stalls in automode
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

Consider these factors:
1. Large codebase with multiple independent components/modules
2. Multiple separate features that can be developed in parallel  
3. Tasks like "implement X, Y, and Z" where X, Y, Z are separable
4. Testing multiple components simultaneously
5. Documentation generation across multiple areas
6. Refactoring that can be divided by file/module boundaries
7. Claude mentions working on multiple files/directories
8. Task involves parallel development streams

Respond with EXACTLY one of:
COORDINATE_BENEFICIAL: [Brief reason why coordination would help]
SINGLE_INSTANCE_SUFFICIENT: [Brief reason why one instance is enough]

Your response:"#,
            claude_message
        );
        
        // Quick local analysis using DeepSeek
        match self.quick_deepseek_analysis(&analysis_prompt).await {
            Ok(response) => {
                tracing::info!("DeepSeek coordination analysis response: {}", response);
                if response.contains("COORDINATE_BENEFICIAL") {
                    tracing::info!("DeepSeek recommends coordination for task");
                    true
                } else {
                    tracing::debug!("DeepSeek says single instance sufficient: {}", response);
                    false
                }
            }
            Err(e) => {
                tracing::warn!("Failed to analyze coordination need: {}", e);
                false
            }
        }
    }
    
    async fn quick_deepseek_analysis(&self, prompt: &str) -> Result<String> {
        let request_body = serde_json::json!({
            "model": "deepseek-r1:8b",
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
            return Err(anyhow::anyhow!("Ollama API error"));
        }
        
        #[derive(serde::Deserialize)]
        struct OllamaResponse {
            response: String,
        }
        
        let ollama_response: OllamaResponse = response.json().await?;
        Ok(ollama_response.response.trim().to_string())
    }
    
    async fn coordinate_multi_instance_task(&mut self, main_instance_id: Uuid, task_description: &str) {
        // Use the default coordination logic
        self.coordinate_multi_instance_task_with_count(main_instance_id, task_description, 0).await;
    }
    
    async fn coordinate_multi_instance_task_with_count(&mut self, main_instance_id: Uuid, task_description: &str, requested_count: usize) {
        tracing::info!("Coordinating multi-instance task: {}", task_description);
        
        // Get current tasks from TaskMaster
        let current_dir = if let Some(instance) = self.instances.iter().find(|i| i.id == main_instance_id) {
            instance.working_directory.clone()
        } else {
            std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".to_string())
        };
        
        // Use DeepSeek to break down the task
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
            task_description,
            current_dir
        );
        
        match self.quick_deepseek_analysis(&breakdown_prompt).await {
            Ok(breakdown) => {
                self.spawn_coordinated_instances_with_count(main_instance_id, &breakdown, &current_dir, requested_count).await;
            }
            Err(e) => {
                tracing::error!("Failed to break down task: {}", e);
                if let Some(main_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                    main_instance.add_message("System".to_string(), 
                        "⚠️ Multi-instance coordination failed - continuing with single instance".to_string());
                }
            }
        }
    }
    
    async fn spawn_coordinated_instances(&mut self, main_instance_id: Uuid, breakdown: &str, working_dir: &str) {
        self.spawn_coordinated_instances_with_count(main_instance_id, breakdown, working_dir, 0).await;
    }
    
    async fn spawn_coordinated_instances_with_count(&mut self, main_instance_id: Uuid, breakdown: &str, working_dir: &str, requested_count: usize) {
        let subtasks: Vec<&str> = breakdown.lines()
            .filter(|line| line.starts_with("SUBTASK_"))
            .collect();
        
        if subtasks.is_empty() {
            tracing::warn!("No subtasks found in breakdown");
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
            
            let instance_name = format!("Claude {}-{}", self.instances.len() + 1, char::from(b'A' + i as u8));
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
            self.instances.push(new_instance);
            
            // Switch to the new instance briefly to show it was created
            if i == 0 {
                self.current_tab = self.instances.len() - 1;
            }
            
            tracing::info!("Spawned coordinated instance {} for subtask: {}", instance_id, task_desc);
        }
        
        // Add final coordination message to main instance
        if let Some(main_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
            main_instance.add_message("System".to_string(), 
                format!("✅ Spawned {} coordinated instances. Use TaskMaster tools to monitor progress across all instances.", subtasks.len()));
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
    'outer: loop {
        // Process any Claude messages
        app.process_claude_messages().await;
        
        // Process any DeepSeek messages
        app.process_deepseek_messages().await;
        
        // Check for stalled conversations
        app.check_for_stalls().await;
        
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
                                    // Track user input for stall detection
                                    instance.on_user_input();
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
                            // Check if click is on any tab using calculated rectangles
                            for (i, tab_rect) in app.tab_rects.iter().enumerate() {
                                if mouse.column >= tab_rect.x 
                                    && mouse.column < tab_rect.x + tab_rect.width
                                    && mouse.row == tab_rect.y {
                                    app.current_tab = i;
                                    app.sync_working_directory();
                                    tracing::info!("Clicked tab {} at ({}, {}) ", i, mouse.column, mouse.row);
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
    if let Some(instance) = app.instances.get_mut(app.current_tab) {
        // Update scroll position based on actual message area height
        let message_area_height = chunks[1].height;
        instance.auto_scroll_to_bottom(Some(message_area_height));
        
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

        let current_dir = if let Ok(home) = std::env::var("HOME") {
            if instance.working_directory.starts_with(&home) {
                instance.working_directory.replacen(&home, "~", 1)
            } else {
                instance.working_directory.clone()
            }
        } else {
            instance.working_directory.clone()
        };
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
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left);
    
    f.render_widget(todo_widget, popup_area);
}

