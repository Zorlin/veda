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
use serde_json::{self, json, Value};
use tui_textarea::TextArea;
use rand::Rng;

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
    // System-generated message (not from actual Claude output)
    is_system_generated: bool,
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

#[derive(Debug, Clone)]
enum BackgroundTask {
    ContinuousTesting,
    CodeQualityChecks,
    PerformanceProfiling,
    SecurityScanning,
    DependencyUpdates,
    DocumentationGeneration,
}

#[derive(Debug, Clone, PartialEq)]
enum SliceState {
    Available,           // Ready for new tasks
    WorkingOnTask,      // Currently working on a user task
    SpawningInstances,  // Spawning other instances for parallel work
    BackgroundWork,     // Performing background maintenance tasks
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
    // Track tools that have been approved after permission denial
    approved_tools: Vec<String>,
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
    // Background task management
    slice_state: SliceState,
    background_task: Option<BackgroundTask>,
    spawned_instances: Vec<Uuid>, // Track instances spawned by this slice
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
            approved_tools: Vec::new(),
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
            slice_state: SliceState::Available,
            background_task: None,
            spawned_instances: Vec::new(),
        }
    }

    fn add_message(&mut self, sender: String, content: String) {
        self.add_message_with_flags(sender, content, false, false, false);
    }
    
    fn add_system_message(&mut self, content: String) {
        self.add_message_with_flags("System".to_string(), content, false, false, true);
    }
    
    fn assign_background_task(&mut self, task: BackgroundTask) {
        self.slice_state = SliceState::BackgroundWork;
        self.background_task = Some(task.clone());
        
        let task_description = match task {
            BackgroundTask::ContinuousTesting => {
                "🧪 **Background Task: Continuous Testing**\n\nI'm now running continuous tests while the main instances work on their tasks. I'll:\n1. Monitor test results continuously\n2. Report any failures immediately\n3. Run different test suites on a rotating basis\n4. Keep the codebase quality high\n\nStarting continuous testing loop..."
            },
            BackgroundTask::CodeQualityChecks => {
                "📊 **Background Task: Code Quality Checks**\n\nI'm now performing code quality analysis while others work. I'll:\n1. Run linting and static analysis\n2. Check code style and formatting\n3. Monitor complexity metrics\n4. Suggest improvements\n\nStarting code quality analysis..."
            },
            BackgroundTask::PerformanceProfiling => {
                "⚡ **Background Task: Performance Profiling**\n\nI'm now monitoring performance while others work. I'll:\n1. Profile critical code paths\n2. Monitor memory usage\n3. Check for performance regressions\n4. Suggest optimizations\n\nStarting performance profiling..."
            },
            BackgroundTask::SecurityScanning => {
                "🔒 **Background Task: Security Scanning**\n\nI'm now scanning for security issues while others work. I'll:\n1. Check for known vulnerabilities\n2. Analyze dependencies for security issues\n3. Review code for security patterns\n4. Monitor for sensitive data exposure\n\nStarting security scanning..."
            },
            BackgroundTask::DependencyUpdates => {
                "📦 **Background Task: Dependency Updates**\n\nI'm now monitoring dependencies while others work. I'll:\n1. Check for available updates\n2. Analyze update compatibility\n3. Monitor for security advisories\n4. Prepare update recommendations\n\nStarting dependency monitoring..."
            },
            BackgroundTask::DocumentationGeneration => {
                "📚 **Background Task: Documentation Generation**\n\nI'm now updating documentation while others work. I'll:\n1. Generate API documentation\n2. Update README files\n3. Create usage examples\n4. Maintain technical documentation\n\nStarting documentation generation..."
            },
        };
        
        self.add_system_message(task_description.to_string());
    }
    
    fn add_message_with_flags(&mut self, sender: String, content: String, is_thinking: bool, is_collapsed: bool, is_system_generated: bool) {
        let timestamp = Local::now().format("%H:%M:%S").to_string();
        self.messages.push(Message {
            timestamp,
            sender,
            content,
            is_thinking,
            is_collapsed,
            is_system_generated,
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
    
    fn auto_scroll_with_width(&mut self, message_area_height: Option<u16>, _terminal_width: Option<u16>) {
        // Simple approach: always scroll to bottom to show latest messages
        // The scroll offset is how many lines to skip from the top
        
        // Count total lines (each message + empty line)
        let total_lines = self.messages.len() * 2; // Each message + separator
        
        // Get visible area height
        let visible_lines = message_area_height.unwrap_or(20).saturating_sub(2) as usize; // Subtract borders
        
        // Calculate scroll offset to show the last visible_lines
        if total_lines > visible_lines {
            // Scroll to show the bottom messages
            self.scroll_offset = (total_lines - visible_lines) as u16;
        } else {
            // All messages fit, no scrolling needed
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
    }
    
    fn get_recent_context(&self) -> (String, String) {
        // Get the last Claude message, excluding system-generated messages
        let claude_message = self.messages.iter()
            .rev()
            .find(|m| m.sender == "Claude" && 
                      !m.content.is_empty() && 
                      !m.is_system_generated)
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
    // Show global aggregated view
    show_global_view: bool,
    // Temporary textarea for global view input
    global_textarea: Option<TextArea<'static>>,
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
        
        // Create the first slice (Slice 0) - nothing special about it
        instances.push(ClaudeInstance::new("Slice 0".to_string()));
        
        let (tx, rx) = mpsc::channel(100);
        let (deepseek_tx, deepseek_rx) = mpsc::channel(100);
        
        // Use the actual PID as the Veda process ID
        let instance_id = std::process::id();
        tracing::info!("Veda process started with PID: {}", instance_id);
        
        Ok(Self {
            instance_id,
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
            show_global_view: true, // Start with global view selected
            global_textarea: None,
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
            
            // Register sessionID -> Veda_PID in shared registry for cross-process coordination
            let veda_pid = self.instance_id; // This is the Veda process PID
            let session_id_for_registry = session_id.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::shared_ipc::RegistryClient::register_session_pid(&session_id_for_registry, veda_pid).await {
                    tracing::warn!("Failed to register session {} -> PID {} in shared registry: {}", session_id_for_registry, veda_pid, e);
                }
            });
            
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
        let slice_num = self.instances.len(); // Zero-based indexing
        let instance_name = format!("Slice {}", slice_num);
        let mut new_instance = ClaudeInstance::new(instance_name.clone());
        
        // Set manually created instances as available for background work
        new_instance.slice_state = SliceState::Available;
        
        tracing::info!("Creating new Veda Slice: {}", instance_name);
        
        self.instances.push(new_instance);
        self.current_tab = self.instances.len() - 1;
        
        // Slice created - session ID will be assigned when user first sends a message
        tracing::info!("✅ New Veda {} created (session ID will be assigned on first use)", instance_name);
    }
    
    fn close_current_instance(&mut self) {
        if self.instances.len() > 1 {
            let instance_id = self.instances[self.current_tab].id;
            
            // Remove this instance from any parent's spawned_instances list
            for parent_instance in self.instances.iter_mut() {
                parent_instance.spawned_instances.retain(|&spawned_id| spawned_id != instance_id);
                
                // If this parent was spawning and now has no spawned instances, update its state
                if parent_instance.slice_state == SliceState::SpawningInstances && parent_instance.spawned_instances.is_empty() {
                    parent_instance.slice_state = SliceState::Available;
                    tracing::debug!("Parent instance {} finished spawning, setting to Available", parent_instance.id);
                }
            }
            
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
            if self.show_global_view {
                // Switch from global to first slice
                self.show_global_view = false;
                self.current_tab = 0;
            } else {
                // Move to next slice
                self.current_tab = (self.current_tab + 1) % self.instances.len();
                if self.current_tab == 0 {
                    // Wrapped around, go to global view
                    self.show_global_view = true;
                }
            }
            
            if !self.show_global_view {
                self.sync_working_directory();
            }
        }
    }

    fn previous_tab(&mut self) {
        if !self.instances.is_empty() {
            if self.show_global_view {
                // Switch from global to last slice
                self.show_global_view = false;
                self.current_tab = self.instances.len() - 1;
            } else if self.current_tab == 0 {
                // At first slice, go to global view
                self.show_global_view = true;
            } else {
                // Move to previous slice
                self.current_tab = self.current_tab - 1;
            }
            
            if !self.show_global_view {
                self.sync_working_directory();
            }
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
  - **Research Mode**: Many TaskMaster tools support `research: true` parameter for Perplexity AI integration
    - Provides up-to-date information, current best practices, and comprehensive analysis
    - Use for: `parse_prd`, `add_task`, `update_task`, `expand_task`, and more
• **Playwright**: Browser automation and testing
  - `mcp__playwright__*` tools for web interaction, testing, and automation
• **DeepWiki**: Repository analysis and documentation
  - `mcp__deepwiki__*` tools for understanding codebases and documentation

**🔬 PERPLEXITY RESEARCH CAPABILITIES:**
TaskMaster AI integrates with Perplexity for enhanced research:
• **PRD Parsing**: Use `research: true` for comprehensive task generation with current tech insights
• **Task Creation**: Research mode provides detailed implementation strategies
• **Task Updates**: Get latest best practices and solutions
• **Complexity Analysis**: Research-backed analysis for better task breakdown

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
- Enable research mode for tasks requiring current information or best practices
- Use Playwright for any web-related testing or automation needs

This prompt appears only once per session. You now have full access to these powerful capabilities!"#.to_string()
    }

    async fn send_message(&mut self, message: String) {
        tracing::info!("send_message called with: {}", message.chars().take(100).collect::<String>());
        
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
        
        // Check if we're in Global view - if so, broadcast to all slices
        if self.show_global_view {
            self.broadcast_to_all_slices(message).await;
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
            if let Err(e) = send_to_claude_with_session(context_message, tx, session_id, process_handle, None).await {
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

    async fn broadcast_to_all_slices(&mut self, message: String) {
        tracing::info!("Broadcasting message from Global view to all slices");
        
        // Collect information about all slices for processing
        let mut slice_infos = Vec::new();
        for (idx, instance) in self.instances.iter().enumerate() {
            let slice_info = (
                idx,
                instance.id,
                instance.name.clone(),
                instance.session_id.clone(),
                instance.is_processing,
                instance.process_handle.clone(),
                instance.working_directory.clone(),
            );
            slice_infos.push(slice_info);
        }
        
        // Add the message to all slices as a user message
        for (idx, _, name, _, _, _, _) in &slice_infos {
            if let Some(instance) = self.instances.get_mut(*idx) {
                instance.add_message("You".to_string(), format!("[Global] {}", message.clone()));
            }
        }
        
        // Process each slice
        for (idx, id, name, session_id, was_processing, process_handle, working_dir) in slice_infos {
            tracing::info!("Broadcasting to {} (Session: {:?}, Processing: {})", 
                         name, session_id, was_processing);
            
            // If the slice is processing, interrupt it first
            if was_processing && process_handle.is_some() {
                if let Some(handle) = process_handle {
                    let mut handle_guard = handle.lock().await;
                    if let Some(ref mut child) = *handle_guard {
                        #[cfg(unix)]
                        {
                            use nix::sys::signal::{self, Signal};
                            use nix::unistd::Pid;
                            
                            if let Some(pid) = child.id() {
                                tracing::info!("Interrupting {} (PID: {}) before broadcasting", name, pid);
                                match signal::kill(Pid::from_raw(pid as i32), Signal::SIGINT) {
                                    Ok(_) => {
                                        if let Some(instance) = self.instances.get_mut(idx) {
                                            instance.add_message("System".to_string(), 
                                                format!("⚡ Interrupted for global broadcast"));
                                            instance.is_processing = false;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to send SIGINT to {}: {}", name, e);
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Wait a moment for the interrupt to take effect
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            
            // Send the message to this slice
            if let Some(session) = session_id {
                // Slice already has a session, resume it with the message
                let tx = self.message_tx.clone();
                let context_message = format!("Working directory: {}\n\n[Global broadcast] {}", working_dir, message);
                
                // Get or create process handle for this slice
                let process_handle = if let Some(instance) = self.instances.get_mut(idx) {
                    instance.is_processing = true;
                    if instance.process_handle.is_none() {
                        let handle = Arc::new(tokio::sync::Mutex::new(None));
                        instance.process_handle = Some(handle.clone());
                        Some(handle)
                    } else {
                        instance.process_handle.clone()
                    }
                } else {
                    None
                };
                
                tokio::spawn(async move {
                    tracing::info!("Sending broadcast to {} with session {:?}", name, session);
                    if let Err(e) = send_to_claude_with_session(
                        context_message,
                        tx,
                        Some(session),
                        process_handle,
                        None
                    ).await {
                        tracing::error!("Error broadcasting to {}: {}", name, e);
                    }
                });
            } else {
                // Slice doesn't have a session yet, start a new one
                let tx = self.message_tx.clone();
                let context_message = format!("Working directory: {}\n\n{}\n\n---\n\n[Global broadcast] {}", 
                                            working_dir, Self::create_capabilities_prompt(), message);
                
                // Create process handle for this slice
                let process_handle = if let Some(instance) = self.instances.get_mut(idx) {
                    instance.is_processing = true;
                    let handle = Arc::new(tokio::sync::Mutex::new(None));
                    instance.process_handle = Some(handle.clone());
                    Some(handle)
                } else {
                    None
                };
                
                let target_id = id;
                tokio::spawn(async move {
                    tracing::info!("Starting new session for {} with broadcast", name);
                    if let Err(e) = send_to_claude_with_session(
                        context_message,
                        tx,
                        None,
                        process_handle,
                        Some(target_id)
                    ).await {
                        tracing::error!("Error starting session for {}: {}", name, e);
                    }
                });
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
        
        // Send SIGINT to the actual Claude process in a non-blocking way
        if let Some(process_handle) = process_handle {
            // Clone the handle for potential background use
            let process_handle_clone = process_handle.clone();
            
            // Try to get the lock without blocking the UI
            if let Ok(mut handle_guard) = process_handle.try_lock() {
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
                // If we can't get the lock immediately, spawn a background task
                tracing::info!("Process handle locked, sending interrupt in background");
                tokio::spawn(async move {
                    let mut handle_guard = process_handle_clone.lock().await;
                    if let Some(ref mut child) = *handle_guard {
                        #[cfg(unix)]
                        {
                            use nix::sys::signal::{self, Signal};
                            use nix::unistd::Pid;
                            
                            if let Some(pid) = child.id() {
                                let pid_u32: u32 = pid;
                                tracing::info!("Background SIGINT to Claude process with PID {}", pid_u32);
                                let _ = signal::kill(Pid::from_raw(pid_u32 as i32), Signal::SIGINT);
                            }
                        }
                    }
                });
                
                if let Some(instance) = self.current_instance_mut() {
                    instance.add_message("System".to_string(), 
                        "📡 Interrupt signal scheduled (background)".to_string());
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
                            false,
                            false  // Not system-generated, this is actual DeepSeek output
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
                                false,
                                false  // Not system-generated, this is actual DeepSeek output
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
                                        if let Err(e) = send_to_claude_with_session(message_to_claude, tx, Some(session_id), None, None).await {
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
            tracing::debug!("Received Claude message: {:?}", msg);
            match msg {
                ClaudeMessage::StreamStart { session_id, .. } => {
                    tracing::info!("StreamStart for session {:?}", session_id);
                    // Don't create empty message - we'll create it when we get actual content
                }
                ClaudeMessage::StreamText { text, session_id } => {
                    tracing::debug!("Processing StreamText message: {} chars, session_id: {:?}", text.len(), session_id);
                    
                    // Find instance by session_id only
                    let target_instance_index = if let Some(session_id_val) = &session_id {
                        let by_session = self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val));
                        if by_session.is_none() {
                            tracing::warn!("Session {} not found in any tab! Available sessions: {:?}", 
                                session_id_val,
                                self.instances.iter().map(|i| i.session_id.as_ref()).collect::<Vec<_>>()
                            );
                        }
                        by_session
                    } else {
                        // No session_id - route to current tab as fallback for system messages
                        Some(self.current_tab)
                    };
                    
                    // If we still can't find the instance and have a session_id, buffer the message
                    if target_instance_index.is_none() && session_id.is_some() {
                        let session_id_val = session_id.as_ref().unwrap();
                        
                        // Add buffer size limit to prevent unbounded growth
                        const MAX_BUFFER_SIZE: usize = 100;
                        if self.pending_session_messages.len() >= MAX_BUFFER_SIZE {
                            tracing::error!("❌ Pending session messages buffer full ({} messages) - dropping message for orphaned session {}", 
                                          MAX_BUFFER_SIZE, session_id_val);
                            tracing::error!("   This session appears to be orphaned. Consider restarting Veda to clear the buffer.");
                            continue;
                        }
                        
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
                    
                    
                    // Hide todo list when new output arrives
                    self.hide_todo_list();
                    
                    if let Some(instance_idx) = target_instance_index {
                        
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
                            // Always trigger auto-scroll for new messages (background tabs get proper dimensions now)
                            instance.auto_scroll_with_width(Some(instance.last_message_area_height), Some(instance.last_terminal_width));
                            // Check if this is todo list data
                            self.parse_todo_list(&text);
                        } else {
                            // Try to append to the last Claude message
                            let needs_todo_parse = if let Some(last_msg) = instance.messages.last_mut() {
                                if last_msg.sender == "Claude" {
                                    last_msg.content.push_str(&text);
                                    // Return the content to parse later
                                    Some(last_msg.content.clone())
                                } else {
                                    // Shouldn't happen based on our check above, but just in case
                                    instance.add_message("Claude".to_string(), text.clone());
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
                            // Add buffer size limit to prevent unbounded growth
                            const MAX_BUFFER_SIZE: usize = 100;
                            if self.pending_session_messages.len() >= MAX_BUFFER_SIZE {
                                tracing::error!("❌ Pending session messages buffer full ({} messages) - dropping message for orphaned session {}", 
                                              MAX_BUFFER_SIZE, session_id_val);
                                tracing::error!("   This session appears to be orphaned. Available instances: {:?}", 
                                    self.instances.iter().map(|i| (i.id, i.name.clone(), i.session_id.clone())).collect::<Vec<_>>());
                            } else {
                                tracing::warn!("⚠️  Failed to route StreamText: session_id={} - buffering message", session_id_val);
                                tracing::warn!("   Available instances: {:?}", 
                                    self.instances.iter().map(|i| (i.id, i.name.clone(), i.session_id.clone())).collect::<Vec<_>>());
                                // Buffer the message for when the session gets established
                                self.pending_session_messages.push((self.instance_id, text.clone(), session_id_val.clone()));
                                tracing::info!("📦 Buffered message for session {} (buffer size: {})", session_id_val, self.pending_session_messages.len());
                            }
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
                        } else {
                            tracing::info!("Task analysis determined coordination not beneficial");
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
                                                
                                                // Enable each tool that Claude needs by sending ToolApproved messages
                                                let mut enabled_tools = Vec::new();
                                                for tool in &tools {
                                                    // Send ToolApproved message instead of using broken claude config command
                                                    let _ = tx.send(ClaudeMessage::ToolApproved {
                                                        tool_name: tool.clone(),
                                                        session_id: Some(session_id.clone()),
                                                    }).await;
                                                    tracing::info!("Successfully approved tool: {}", tool);
                                                    enabled_tools.push(tool.clone());
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
                                                    
                                                    if let Err(e) = send_to_claude_with_session(response, tx, Some(session_id), None, None).await {
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
                                            if let Err(e) = send_to_claude_with_session(coordination_response.to_string(), tx.clone(), Some(session_id.clone()), None, None).await {
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
                ClaudeMessage::SystemMessage { text, session_id } => {
                    // Handle system-generated messages (like spawn confirmations)
                    let target_instance_index = if let Some(session_id_val) = &session_id {
                        self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                    } else {
                        Some(self.current_tab)
                    };
                    
                    if let Some(instance_idx) = target_instance_index {
                        let instance = &mut self.instances[instance_idx];
                        instance.add_system_message(text);
                        // Trigger auto-scroll for system messages
                        instance.auto_scroll_with_width(Some(instance.last_message_area_height), Some(instance.last_terminal_width));
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
                ClaudeMessage::SessionStarted { session_id, target_tab_id } => {
                    tracing::info!("🎬 SessionStarted received for session_id: {}", session_id);
                    
                    let target_instance_index = if let Some(tab_id) = target_tab_id {
                        // Find instance by tab ID
                        let idx = self.instances.iter().position(|i| i.id == tab_id);
                        tracing::info!("🔄 Target tab ID specified: {}, found instance: {:?}", tab_id, idx);
                        idx
                    } else {
                        // Fallback: find first instance without a session_id
                        let idx = self.instances.iter().position(|i| i.session_id.is_none());
                        tracing::info!("🔄 No target specified, using fallback assignment: {:?}", idx);
                        idx
                    };
                    
                    // Only assign session if we found a valid instance
                    if let Some(instance_idx) = target_instance_index {
                        // Check if we're within the max instances limit
                        if instance_idx < self.max_instances {
                            self.assign_session_to_instance(Some(instance_idx), session_id);
                        } else {
                            tracing::error!("❌ Cannot assign session {} - instance index {} exceeds max_instances limit of {}", 
                                          session_id, instance_idx, self.max_instances);
                        }
                    } else {
                        tracing::error!("❌ Cannot assign session {} - no available instance found (all {} instances have sessions)", 
                                      session_id, self.instances.len());
                        // If we're at max capacity, log additional info
                        if self.instances.len() >= self.max_instances {
                            tracing::error!("   Already at maximum instance capacity ({}/{})", 
                                          self.instances.len(), self.max_instances);
                        }
                    }
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
                            let target_instance_index_copy = target_instance_index;
                            
                            tokio::spawn(async move {
                                tracing::info!("Automode: Analyzing safety of tool: {}", tool_name_copy);
                                
                                match Self::analyze_tool_safety(&tool_name_copy).await {
                                    Ok(true) => {
                                        tracing::info!("DeepSeek approved enabling tool: {}", tool_name_copy);
                                        
                                        // Instead of trying to enable the tool via Claude CLI (which doesn't work),
                                        // we'll just track that it's approved and notify Claude after restart
                                        tracing::info!("Tool {} approved by DeepSeek safety analysis", tool_name_copy);
                                        
                                        // Send tool approval message to main app
                                        let _ = tx.send(ClaudeMessage::ToolApproved {
                                            tool_name: tool_name_copy.clone(),
                                            session_id: session_id_copy.clone(),
                                        }).await;
                                        
                                        let _ = tx.send(ClaudeMessage::StreamText {
                                            text: format!("🔧 Automode: Tool {} approved and will be available after restart", tool_name_copy),
                                            session_id: session_id_copy.clone(),
                                        }).await;
                                            
                                            // Kill the current process if it exists
                                            if process_handle.is_none() {
                                                tracing::error!("No process handle available for tool enablement interrupt!");
                                            }
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
                                            // Note: We can't access self.instances here in the spawned task,
                                            // so we'll use a simple message for now
                                            let response = format!("I've enabled the {} tool for you. Please try using it again.", tool_name_copy);
                                            
                                            // Create a new process handle for the resumed session
                                            let new_handle = Arc::new(tokio::sync::Mutex::new(None));
                                            
                                            // Send message to update the instance's process handle in the main App
                                            let _ = tx.send(ClaudeMessage::ProcessHandleUpdate {
                                                session_id: session_id_copy.clone(),
                                                process_handle: new_handle.clone(),
                                            }).await;
                                            
                                            let _ = tx.send(ClaudeMessage::StreamText {
                                                text: format!("📝 Resuming session after enabling tool: {}", tool_name_copy),
                                                session_id: session_id_copy.clone(),
                                            }).await;
                                            
                                            tracing::info!("Resuming session {:?} with tool {} enabled", session_id_copy, tool_name_copy);
                                            if let Err(e) = send_to_claude_with_session(response, tx.clone(), session_id_copy.clone(), Some(new_handle.clone()), None).await {
                                                tracing::error!("Failed to resume session {:?} with tool enablement: {}", session_id_copy, e);
                                            } else {
                                                tracing::info!("Successfully initiated session resume for {:?} with tool {} enabled", session_id_copy, tool_name_copy);
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
                ClaudeMessage::ToolApproved { tool_name, session_id } => {
                    tracing::info!("Tool {} approved for session {:?}", tool_name, session_id);
                    
                    // Find instance by session_id and add to approved tools list
                    let target_instance_index = if let Some(session_id_val) = &session_id {
                        self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                    } else {
                        Some(0) // Fallback to first instance
                    };
                    
                    if let Some(index) = target_instance_index {
                        let instance = &mut self.instances[index];
                        // Add to approved tools list if not already there
                        if !instance.approved_tools.contains(&tool_name) {
                            instance.approved_tools.push(tool_name.clone());
                            tracing::info!("Added '{}' to approved tools list for session {:?}", tool_name, session_id);
                        }
                        // Also add to successful tools to avoid future permission checks
                        if !instance.successful_tools.contains(&tool_name) {
                            instance.successful_tools.push(tool_name.clone());
                        }
                    }
                }
                ClaudeMessage::VedaSpawnInstances { task_description, num_instances, session_id } => {
                    tracing::info!("Claude requested to spawn {} Veda Slices for task: {} (session: {})", num_instances, task_description, session_id);
                    
                    // Find the exact source instance using the session ID - no fallbacks, no guessing
                    let source_instance_index = self.instances.iter().position(|i| i.session_id.as_ref() == Some(&session_id))
                        .expect(&format!("Session {} must exist in instances for spawning", session_id));
                    
                    // Mark the source instance as spawning and assign background work to available slices
                    self.instances[source_instance_index].slice_state = SliceState::SpawningInstances;
                    self.assign_background_work_if_available();
                    
                    // Check capacity before proceeding
                    let current_slices = self.instances.len();
                    let max_slices = self.max_instances;
                    let available_slots = if current_slices < max_slices {
                        max_slices - current_slices
                    } else {
                        0
                    };
                    
                    if num_instances as usize > available_slots {
                        // Cannot spawn requested number of slices
                        let error_msg = if available_slots > 0 {
                            format!("❌ Cannot spawn {} Veda Slices. Currently at {}/{} slices. You can spawn up to {} more slice(s).", 
                                    num_instances, current_slices, max_slices, available_slots)
                        } else {
                            format!("❌ Cannot spawn any Veda Slices. Already at maximum capacity ({}/{} slices).", 
                                    current_slices, max_slices)
                        };
                        
                        self.instances[source_instance_index].add_message("Tool".to_string(), error_msg);
                        tracing::warn!("Rejecting spawn request: {} Veda Slices requested, {} available slots", num_instances, available_slots);
                        return; // Exit early, don't proceed with spawning
                    }
                    
                    let coord_instance_id = self.instances[source_instance_index].id;
                    self.instances[source_instance_index].add_message("Tool".to_string(), 
                        format!("🤝 Spawning {} additional Veda Slices for task: {}", num_instances, task_description));
                    
                    tracing::info!("Using Veda Slice {} (tab {}) as coordination source for spawning", coord_instance_id, source_instance_index);
                    
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
                    
                    // Processing message already added above when we identified the source instance
                    
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
                        
                        // Perform the analysis with reasonable timeout for Ollama (much faster than DeepSeek)
                        let analysis_timeout = tokio::time::Duration::from_secs(60); // 1 minute max for coordination
                        match tokio::time::timeout(analysis_timeout, perform_gemma_analysis(&breakdown_prompt)).await {
                            Ok(Ok(breakdown)) => {
                                tracing::info!("Background analysis completed, sending InternalCoordinateInstances message");
                                if let Err(e) = tx.send(ClaudeMessage::InternalCoordinateInstances {
                                    main_instance_id: coord_instance_id,
                                    task_description: breakdown,
                                    num_instances: num_instances_clone as usize,
                                    working_dir: current_dir,
                                    is_ipc: false, // All spawning is now direct, not IPC
                                }).await {
                                    tracing::error!("Failed to send InternalCoordinateInstances message: {}", e);
                                } else {
                                    tracing::info!("Successfully sent InternalCoordinateInstances message");
                                }
                            }
                            Ok(Err(e)) => {
                                tracing::error!("Background coordination analysis failed: {}", e);
                                
                                // Always show the error to the user and fail gracefully
                                let error_msg = if e.to_string().contains("Missing Ollama model 'gemma3:12b'") {
                                    e.to_string()
                                } else {
                                    format!("❌ SPAWN FAILED: Ollama analysis error: {}\n\nSpawning requires successful task analysis from Ollama. Please check your Ollama setup and try again.", e)
                                };
                                
                                if let Err(send_err) = tx.send(ClaudeMessage::StreamText {
                                    text: error_msg,
                                    session_id: None, // Will be routed to the main tab
                                }).await {
                                    tracing::error!("Failed to send analysis error message: {}", send_err);
                                }
                                return; // Don't spawn instances without proper analysis
                            }
                            Err(_) => {
                                tracing::error!("Ollama coordination analysis timed out after 1 minute");
                                
                                // Always fail gracefully when timeout occurs - no fallback spawning
                                let error_msg = format!(
                                    "❌ SPAWN FAILED: Ollama analysis timed out after 1 minute\n\nTask analysis is required for intelligent instance spawning. Please ensure Ollama is running and responsive, then try again."
                                );
                                
                                if let Err(send_err) = tx.send(ClaudeMessage::StreamText {
                                    text: error_msg,
                                    session_id: None, // Will be routed to the main tab
                                }).await {
                                    tracing::error!("Failed to send timeout error message: {}", send_err);
                                }
                                return; // Don't spawn instances without proper analysis
                            }
                        }
                    });
                }
                ClaudeMessage::VedaListInstances { session_id } => {
                    tracing::info!("Claude requested instance list (session: {})", session_id);
                    
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
                    
                    // Find the source instance by session ID and send the message there
                    let instance = self.instances.iter_mut().find(|i| i.session_id.as_ref() == Some(&session_id))
                        .expect(&format!("Session {} must exist in instances for list request", session_id));
                    instance.add_message("Tool".to_string(), message);
                }
                ClaudeMessage::VedaCloseInstance { session_id } => {
                    tracing::info!("Claude requested to close its own instance (session: {})", session_id);
                    
                    // Find the instance that made the request
                    let target_index = self.instances.iter().position(|i| i.session_id.as_ref() == Some(&session_id))
                        .expect(&format!("Session {} must exist in instances for close request", session_id));
                    
                    let result_message = if target_index == 0 {
                        "❌ Cannot close the main instance (Tab 1)".to_string()
                    } else if self.instances.len() <= 1 {
                        "❌ Cannot close the last remaining instance".to_string()
                    } else {
                        let closed_name = self.instances[target_index].name.clone();
                        
                        // Send confirmation message before removing the instance
                        self.instances[target_index].add_message("Tool".to_string(), 
                            format!("✅ Closing instance: {}", closed_name));
                        
                        self.instances.remove(target_index);
                        
                        // Adjust current tab if necessary
                        if self.current_tab >= self.instances.len() {
                            self.current_tab = self.instances.len() - 1;
                        } else if self.current_tab > target_index {
                            self.current_tab -= 1;
                        }
                        
                        self.sync_working_directory();
                        tracing::info!("Successfully closed instance: {}", closed_name);
                        return; // Instance is gone, no need to send result message
                    };
                    
                    // Only send error messages (success case returns early)
                    let instance = self.instances.iter_mut().find(|i| i.session_id.as_ref() == Some(&session_id))
                        .expect(&format!("Session {} must exist for error message", session_id));
                    instance.add_message("Tool".to_string(), result_message);
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
                ClaudeMessage::ProcessHandleUpdate { session_id, process_handle } => {
                    tracing::info!("Updating process handle for session {:?}", session_id);
                    
                    // Find instance by session_id and update its process handle
                    if let Some(session_id_val) = session_id {
                        if let Some(instance) = self.instances.iter_mut().find(|i| i.session_id.as_ref() == Some(&session_id_val)) {
                            instance.process_handle = Some(process_handle);
                            tracing::info!("Successfully updated process handle for session {}", session_id_val);
                        } else {
                            tracing::warn!("Could not find instance with session_id {} to update process handle", session_id_val);
                        }
                    } else {
                        tracing::warn!("ProcessHandleUpdate received without session_id");
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
        
        // Gather context for comprehensive analysis
        let initial_user_prompt = self.get_initial_user_prompt();
        let recent_conversation = self.get_recent_conversation_context();
        let taskmaster_context = self.get_taskmaster_context().await;
        
        // Use Ollama analysis (not the entire conversation, just recent context + initial prompt)
        tracing::info!("Analyzing coordination potential with Ollama - Recent conversation: {} chars, Initial prompt: {} chars, TaskMaster: {} chars", 
                      recent_conversation.len(), 
                      initial_user_prompt.as_ref().map_or(0, |s| s.len()),
                      taskmaster_context.len());
        
        let analysis_prompt = format!(
            r#"Analyze if this task would benefit from multiple parallel Claude Code instances working together.

INITIAL USER REQUEST:
{initial_prompt}

RECENT CONVERSATION (last 3 exchanges):
{recent_conversation}

TASKMASTER PROJECT STATE:
{taskmaster_state}

Consider these factors for PARALLEL INSTANCES (respond COORDINATE_BENEFICIAL if ANY apply):
1. Multiple independent components/modules that can be worked on separately
2. Multiple separate features that can be developed in parallel  
3. Tasks like "implement X, Y, and Z" where X, Y, Z are separable and independent
4. Testing multiple components simultaneously without interference
5. Documentation generation across multiple independent areas
6. Refactoring that can be divided by file/module boundaries
7. Multiple files/directories mentioned that can be worked on independently
8. Task involves parallel development streams
9. TaskMaster shows multiple pending tasks that could be parallelized
10. Initial user request mentions multiple independent objectives

ANALYSIS CRITERIA:
- Focus on the initial request and recent conversation context
- Look for natural separation boundaries in the work
- Identify if multiple independent work streams exist
- Assess if tasks can be done simultaneously without conflicts
- Consider TaskMaster tasks that could be parallelized

IMPORTANT: Independent, separable tasks are IDEAL for parallel instances!

Respond with EXACTLY one of:
COORDINATE_BENEFICIAL: [Brief reason - focus on independence and separability]
SINGLE_INSTANCE_SUFFICIENT: [Brief reason - only if tasks are tightly coupled/interdependent]

Your response:"#,
            initial_prompt = initial_user_prompt.as_ref().map_or("No initial prompt found".to_string(), |p| p.clone()),
            recent_conversation = recent_conversation,
            taskmaster_state = taskmaster_context
        );
        
        // Quick local analysis using Ollama/Gemma with timeout protection
        let analysis_timeout = tokio::time::Duration::from_secs(60); // Allow up to 60 seconds for analysis
        match tokio::time::timeout(analysis_timeout, self.quick_deepseek_analysis(&analysis_prompt)).await {
            Ok(Ok(response)) => {
                tracing::info!("Ollama coordination analysis response: {}", response);
                if response.contains("COORDINATE_BENEFICIAL") {
                    tracing::info!("Ollama recommends coordination for task");
                    true
                } else {
                    tracing::debug!("Ollama says single instance sufficient: {}", response);
                    false
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Ollama analysis failed: {}, skipping coordination", e);
                false
            }
            Err(_) => {
                tracing::warn!("Ollama analysis timed out after 60s, skipping coordination");
                false
            }
        }
    }

    fn get_initial_user_prompt(&self) -> Option<String> {
        // Look for the first user message in the current instance's conversation
        if let Some(instance) = self.current_instance() {
            for message in &instance.messages {
                if message.sender == "You" && !message.content.trim().is_empty() {
                    return Some(message.content.clone());
                }
            }
        }
        None
    }

    fn get_recent_conversation_context(&self) -> String {
        // Get the last 3 back-and-forth exchanges (up to 6 messages total)
        if let Some(instance) = self.current_instance() {
            let messages: Vec<String> = instance.messages
                .iter()
                .rev()  // Start from most recent
                .take(6)  // Take last 6 messages max
                .filter(|msg| !msg.is_system_generated && (msg.sender == "You" || msg.sender == "Claude"))
                .map(|msg| format!("{}: {}", msg.sender, msg.content.chars().take(500).collect::<String>()))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()  // Reverse back to chronological order
                .collect();

            if messages.is_empty() {
                "No recent conversation found".to_string()
            } else {
                format!("Recent conversation (last {} messages):\n{}", messages.len(), messages.join("\n\n"))
            }
        } else {
            "No conversation context available".to_string()
        }
    }

    async fn get_taskmaster_context(&self) -> String {
        // Try to get current TaskMaster tasks for context
        // This is a basic implementation - in practice, you might want to use MCP tools
        let working_dir = if let Some(instance) = self.current_instance() {
            &instance.working_directory
        } else {
            "."
        };

        // Check if there's a tasks.json file we can read
        let tasks_path = format!("{}/tasks/tasks.json", working_dir);
        if let Ok(tasks_content) = std::fs::read_to_string(&tasks_path) {
            // Parse and summarize the tasks
            if let Ok(tasks_json) = serde_json::from_str::<serde_json::Value>(&tasks_content) {
                if let Some(tasks_array) = tasks_json.get("tasks").and_then(|t| t.as_array()) {
                    let mut summary = format!("Found {} TaskMaster tasks:\n", tasks_array.len());
                    
                    for (i, task) in tasks_array.iter().take(10).enumerate() { // Limit to first 10 tasks
                        if let (Some(title), Some(status)) = (
                            task.get("title").and_then(|t| t.as_str()),
                            task.get("status").and_then(|s| s.as_str())
                        ) {
                            summary.push_str(&format!("{}. [{}] {}\n", i + 1, status.to_uppercase(), title));
                        }
                    }
                    
                    if tasks_array.len() > 10 {
                        summary.push_str(&format!("... and {} more tasks\n", tasks_array.len() - 10));
                    }
                    
                    return summary;
                }
            }
        }

        // If no TaskMaster tasks found, check for project structure hints
        let readme_paths = [
            format!("{}/README.md", working_dir),
            format!("{}/readme.md", working_dir),
            format!("{}/README.txt", working_dir),
        ];

        for readme_path in &readme_paths {
            if let Ok(readme_content) = std::fs::read_to_string(readme_path) {
                // Extract first few lines for context
                let lines: Vec<&str> = readme_content.lines().take(5).collect();
                if !lines.is_empty() {
                    return format!("Project README context:\n{}", lines.join("\n"));
                }
            }
        }

        "No TaskMaster tasks or project context found".to_string()
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
                        let status = response.status();
                        tracing::warn!("Ollama API error: status {}", status);
                        
                        // Handle 404 as a specific case for missing model
                        if status == reqwest::StatusCode::NOT_FOUND {
                            return Err(anyhow::anyhow!(
                                "❌ SPAWN FAILED: Missing Ollama model 'gemma3:12b'\n\n\
                                To use Veda's multi-instance spawning feature, you need to install the gemma3:12b model:\n\
                                \n\
                                Run this command in your terminal:\n\
                                ollama pull gemma3:12b\n\
                                \n\
                                This model is used for intelligent task breakdown and coordination between Claude instances.\n\
                                Without it, spawning additional instances will not work."
                            ));
                        }
                        
                        if retry_count >= max_retries {
                            return Err(anyhow::anyhow!("Ollama API error after {} retries: status {}", max_retries, status));
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
        tracing::warn!("Ollama breakdown analysis result: {:?}", breakdown);
        tracing::warn!("Extracted subtasks: {:?}", subtasks);
        
        if subtasks.is_empty() {
            tracing::error!("No valid subtasks found in breakdown. Cannot spawn instances without proper task analysis.");
            tracing::warn!("Received breakdown: {:?}", breakdown);
            
            // Add message to main instance explaining the failure
            if let Some(main_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                main_instance.add_message("System".to_string(), 
                    "❌ Spawning failed: No valid subtasks found in Ollama analysis. Please ensure Ollama is working properly.".to_string());
            }
            return; // Don't spawn instances without proper task breakdown
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
        let starting_count = self.instances.len();
        for i in 0..instances_to_spawn {
            if self.instances.len() >= self.max_instances {
                break;
            }
            
            let subtask = subtasks.get(i % subtasks.len()).unwrap_or(&"General coordination task");
            
            let instance_name = format!("Slice {}", starting_count + i); // Zero-based indexing
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
            
            // Set the new instance's slice state as working on task
            new_instance.slice_state = SliceState::WorkingOnTask;
            
            self.instances.push(new_instance);
            
            // Track this instance as spawned by the parent
            if let Some(parent_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                parent_instance.spawned_instances.push(instance_id);
                tracing::debug!("Tracked instance {} as spawned by parent {}", instance_id, main_instance_id);
            }
            
            // Create and store process handle for the spawned instance
            let process_handle = Arc::new(tokio::sync::Mutex::new(None));
            self.instances.last_mut().unwrap().process_handle = Some(process_handle.clone());
            
            // Switch to the new instance briefly to show it was created
            if i == 0 {
                self.current_tab = self.instances.len() - 1;
            }
            
            tracing::info!("Spawned coordinated instance {} for subtask: {}", instance_id, task_desc);
            
            // Auto-start the instance with its task in the background
            let tx = self.message_tx.clone();
            let task_instruction = format!(
                "You are working on: {}\n\nYour specific assignment: {}\nScope: {}\nPriority: {}\n\nStart by understanding the codebase and focusing on your assigned work. Use TaskMaster tools to coordinate with other instances.",
                breakdown, task_desc, scope, priority
            );
            
            let instance_id_copy = instance_id;
            let instance_name_copy2 = instance_name_copy.clone();
            // Capture coordinator's session ID for status messages
            let coordinator_session_id = self.instances.iter()
                .find(|i| i.id == main_instance_id)
                .map(|i| i.session_id.clone())
                .unwrap_or_else(|| self.instances[0].session_id.clone());
            
            // Start the Claude process for this instance automatically
            tokio::spawn(async move {
                // Wait a moment to ensure the UI has been updated
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                
                // Pre-enable essential tools for the spawned instance by sending ToolApproved messages
                let essential_tools = ["Edit", "MultiEdit", "Read", "Write", "Bash", "TodoRead", "TodoWrite", "Glob", "Grep", "LS"];
                for tool in essential_tools.iter() {
                    // Send ToolApproved message instead of using broken claude config command
                    let _ = tx.send(ClaudeMessage::ToolApproved {
                        tool_name: tool.to_string(),
                        session_id: None, // For spawned instances, session_id will be set later
                    }).await;
                    tracing::debug!("Pre-approved tool {} for spawned instance", tool);
                }
                
                tracing::info!("Auto-starting Claude Code instance {} ({}) with task", instance_name_copy2, instance_id_copy);
                
                // Spawn Claude Code instance with the task instruction and process handle
                let spawn_result = crate::claude::send_to_claude_with_session(
                    task_instruction.clone(),
                    tx.clone(),
                    None, // No existing session for new instance
                    Some(process_handle), // Pass the process handle
                    Some(instance_id_copy), // Target tab ID for session assignment
                ).await;
                
                match spawn_result {
                    Ok(()) => {
                        tracing::info!("✅ Successfully started Claude Code instance for {}", instance_name_copy2);
                        
                        // Send success message to coordinator instance  
                        let _ = tx.send(ClaudeMessage::SystemMessage {
                            text: format!("✅ Started Claude Code instance {} with task", instance_name_copy2),
                            session_id: coordinator_session_id.clone(),
                        }).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to start Claude Code instance for {}: {}", instance_name_copy2, e);
                        // Send error message to coordinator instance
                        let _ = tx.send(ClaudeMessage::SystemMessage {
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
                
                // Create and store process handle before spawning
                let process_handle = Arc::new(tokio::sync::Mutex::new(None));
                self.instances[0].process_handle = Some(process_handle.clone());
                
                tokio::spawn(async move {
                    // Wait a moment for the spawning messages to complete
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    
                    tracing::info!("Auto-starting main instance with coordination task");
                    if let Err(e) = send_to_claude_with_session(main_task_instruction, tx, main_session_id, Some(process_handle), None).await {
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

    /// Select an appropriate background task based on current project needs
    fn select_background_task(&self) -> BackgroundTask {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        // Use a simple rotation based on current time to distribute tasks
        let mut hasher = DefaultHasher::new();
        chrono::Local::now().timestamp().hash(&mut hasher);
        let task_index = (hasher.finish() % 6) as usize;
        
        match task_index {
            0 => BackgroundTask::ContinuousTesting,
            1 => BackgroundTask::CodeQualityChecks,
            2 => BackgroundTask::PerformanceProfiling,
            3 => BackgroundTask::SecurityScanning,
            4 => BackgroundTask::DependencyUpdates,
            _ => BackgroundTask::DocumentationGeneration,
        }
    }

    /// Find an available slice for background work (prioritize non-active slices)
    fn find_available_slice_for_background_work(&mut self) -> Option<usize> {
        // First, look for completely idle slices (Available state)
        for (idx, instance) in self.instances.iter().enumerate() {
            match instance.slice_state {
                SliceState::Available => {
                    // Skip the current active tab unless no other options
                    if idx != self.current_tab {
                        return Some(idx);
                    }
                }
                _ => {}
            }
        }
        
        // If no idle slices, check if the current tab is available
        if let Some(current_instance) = self.instances.get(self.current_tab) {
            if matches!(current_instance.slice_state, SliceState::Available) {
                return Some(self.current_tab);
            }
        }
        
        None
    }

    /// Assign background work to an available slice when others start spawning
    fn assign_background_work_if_available(&mut self) {
        if let Some(slice_idx) = self.find_available_slice_for_background_work() {
            let background_task = self.select_background_task();
            let task_name = match background_task {
                BackgroundTask::ContinuousTesting => "Continuous Testing",
                BackgroundTask::CodeQualityChecks => "Code Quality Checks", 
                BackgroundTask::PerformanceProfiling => "Performance Profiling",
                BackgroundTask::SecurityScanning => "Security Scanning",
                BackgroundTask::DependencyUpdates => "Dependency Updates",
                BackgroundTask::DocumentationGeneration => "Documentation Generation",
            };
            
            tracing::info!("Assigning background task '{}' to slice {} while others spawn instances", task_name, slice_idx);
            
            if let Some(instance) = self.instances.get_mut(slice_idx) {
                instance.assign_background_task(background_task.clone());
                
                // Start the background work by sending the appropriate prompt
                let tx = self.message_tx.clone();
                let session_id = instance.session_id.clone();
                let background_prompt = self.create_background_task_prompt(&background_task);
                
                tokio::spawn(async move {
                    if let Err(e) = crate::claude::send_to_claude_with_session(
                        background_prompt,
                        tx,
                        session_id,
                        None, // No process handle needed for background tasks
                        None,
                    ).await {
                        tracing::error!("Failed to start background task: {}", e);
                    }
                });
            }
        } else {
            tracing::debug!("No available slices for background work assignment");
        }
    }

    /// Create a detailed prompt for the background task
    fn create_background_task_prompt(&self, task: &BackgroundTask) -> String {
        let working_dir = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());
        
        match task {
            BackgroundTask::ContinuousTesting => format!(
                "🧪 **Background Task: Continuous Testing**\n\n\
                While other instances work on development tasks, I need to continuously monitor and test the codebase.\n\n\
                Working directory: {}\n\n\
                My responsibilities:\n\
                1. Run automated tests every few minutes\n\
                2. Monitor for test failures and report immediately\n\
                3. Run different test suites (unit, integration, etc.) on rotation\n\
                4. Check test coverage and suggest improvements\n\
                5. Report any build issues or compilation errors\n\n\
                I should start by examining the project structure to understand what tests are available, then begin a continuous testing loop.\n\n\
                Start working now - begin by exploring the test structure.",
                working_dir
            ),
            BackgroundTask::CodeQualityChecks => format!(
                "📊 **Background Task: Code Quality Analysis**\n\n\
                While other instances work on development tasks, I need to continuously monitor code quality.\n\n\
                Working directory: {}\n\n\
                My responsibilities:\n\
                1. Run linting tools (clippy, etc.) regularly\n\
                2. Check code formatting and style consistency\n\
                3. Monitor complexity metrics and suggest refactoring\n\
                4. Review code for best practices and patterns\n\
                5. Generate code quality reports\n\n\
                I should start by examining the codebase structure and available quality tools, then begin monitoring.\n\n\
                Start working now - begin by analyzing the current code quality.",
                working_dir
            ),
            BackgroundTask::PerformanceProfiling => format!(
                "⚡ **Background Task: Performance Monitoring**\n\n\
                While other instances work on development tasks, I need to monitor and profile performance.\n\n\
                Working directory: {}\n\n\
                My responsibilities:\n\
                1. Profile critical code paths for performance bottlenecks\n\
                2. Monitor memory usage and detect leaks\n\
                3. Benchmark key operations regularly\n\
                4. Check for performance regressions\n\
                5. Suggest optimizations and improvements\n\n\
                I should start by understanding the performance-critical parts of the system, then begin profiling.\n\n\
                Start working now - begin by identifying performance hotspots.",
                working_dir
            ),
            BackgroundTask::SecurityScanning => format!(
                "🔒 **Background Task: Security Scanning**\n\n\
                While other instances work on development tasks, I need to continuously scan for security issues.\n\n\
                Working directory: {}\n\n\
                My responsibilities:\n\
                1. Scan for known security vulnerabilities\n\
                2. Check dependencies for security advisories\n\
                3. Review code for security anti-patterns\n\
                4. Monitor for sensitive data exposure\n\
                5. Generate security reports and recommendations\n\n\
                I should start by examining the dependencies and code for security issues, then begin monitoring.\n\n\
                Start working now - begin by scanning for security vulnerabilities.",
                working_dir
            ),
            BackgroundTask::DependencyUpdates => format!(
                "📦 **Background Task: Dependency Management**\n\n\
                While other instances work on development tasks, I need to monitor and manage dependencies.\n\n\
                Working directory: {}\n\n\
                My responsibilities:\n\
                1. Check for available dependency updates\n\
                2. Analyze update compatibility and breaking changes\n\
                3. Monitor for security advisories in dependencies\n\
                4. Prepare update recommendations with impact analysis\n\
                5. Maintain dependency health reports\n\n\
                I should start by examining the current dependencies and their versions, then begin monitoring.\n\n\
                Start working now - begin by analyzing current dependencies.",
                working_dir
            ),
            BackgroundTask::DocumentationGeneration => format!(
                "📚 **Background Task: Documentation Maintenance**\n\n\
                While other instances work on development tasks, I need to maintain and update documentation.\n\n\
                Working directory: {}\n\n\
                My responsibilities:\n\
                1. Generate and update API documentation\n\
                2. Maintain README files and usage guides\n\
                3. Create and update code examples\n\
                4. Keep technical documentation current\n\
                5. Generate documentation reports and suggestions\n\n\
                I should start by examining the current documentation state and identifying gaps, then begin updating.\n\n\
                Start working now - begin by reviewing documentation completeness.",
                working_dir
            ),
        }
    }

    fn stop_background_work_for_instance(&mut self, instance_id: Uuid) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
            if instance.slice_state == SliceState::BackgroundWork {
                instance.slice_state = SliceState::Available;
                instance.background_task = None;
                
                // Send a message to the instance to stop background work
                instance.add_message("System".to_string(), 
                    "🛑 Background work assignment cancelled. You are now available for new tasks.".to_string());
                
                tracing::info!("Stopped background work for instance {}", instance_id);
            }
        }
    }
}

async fn start_ipc_server(app_tx: mpsc::Sender<ClaudeMessage>) {
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
                    
                    // Note: The actual capacity check happens in the VedaSpawnInstances handler
                    // which has access to the app state and can check current slice count
                    
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
                        task_description: task_desc.to_string(),
                        num_instances,
                        session_id: session_id.to_string(),
                    }).await;
                    
                    format!("✅ Request to spawn {} instances sent for task: {}", num_instances, task_desc)
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
                        session_id: session_id.to_string(),
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

// Standalone function for background Ollama analysis
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
    
    // Enhanced retry with exponential backoff - more aggressive retry for critical spawning
    let mut retry_count = 0;
    let max_retries = 5; // Increased retries for better reliability
    
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
                    let status = response.status();
                    tracing::warn!("Ollama API error: status {}", status);
                    
                    // Handle 404 as a specific case for missing model
                    if status == reqwest::StatusCode::NOT_FOUND {
                        return Err(anyhow::anyhow!(
                            "❌ SPAWN FAILED: Missing Ollama model 'gemma3:12b'\n\n\
                            To use Veda's multi-instance spawning feature, you need to install the gemma3:12b model:\n\
                            \n\
                            Run this command in your terminal:\n\
                            ollama pull gemma3:12b\n\
                            \n\
                            This model is used for intelligent task breakdown and coordination between Claude instances.\n\
                            Without it, spawning additional instances will not work."
                        ));
                    }
                    
                    if retry_count >= max_retries {
                        return Err(anyhow::anyhow!("Ollama API error after {} retries: status {}", max_retries, status));
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to contact Ollama (attempt {}/{}): {}", retry_count + 1, max_retries, e);
                if retry_count >= max_retries {
                    // Check if it's a connection error
                    if e.to_string().contains("Connection refused") || e.to_string().contains("error trying to connect") {
                        return Err(anyhow::anyhow!(
                            "❌ SPAWN FAILED: Cannot connect to Ollama\n\n\
                            Ollama is not running. To use Veda's multi-instance spawning feature:\n\
                            \n\
                            1. Install Ollama from https://ollama.ai\n\
                            2. Start Ollama by running: ollama serve\n\
                            3. Install the required model: ollama pull gemma3:12b\n\
                            \n\
                            Without Ollama, spawning additional Veda Slices will not work."
                        ));
                    } else {
                        return Err(anyhow::anyhow!("Failed to contact Ollama after {} retries: {}", max_retries, e));
                    }
                }
            }
        }
        
        // Enhanced exponential backoff: 2s, 4s, 8s, 16s, 32s with jitter
        retry_count += 1;
        let base_delay = std::cmp::min(2u64 << (retry_count - 1), 30); // Cap at 30s for faster recovery
        // Add jitter to prevent thundering herd if multiple processes retry
        let jitter = rand::thread_rng().gen_range(0..1000); // 0-999ms jitter
        let delay_ms = (base_delay * 1000) + jitter;
        tracing::info!("Retrying Ollama request in {:.1}s (attempt {}/{})...", delay_ms as f64 / 1000.0, retry_count, max_retries);
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Check if we're running in MCP server mode
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--mcp-server" {
        return run_mcp_server().await;
    }
    
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
        .with_env_filter("debug")  // More permissive: log debug level for all modules
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
    
    // Start the shared registry server (only one instance across all Veda processes)
    // If it's already running, this will fail silently which is expected
    let app_tx_for_registry = app.message_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::shared_ipc::start_shared_ipc_server(Some(app_tx_for_registry)).await {
            // Only log if it's not "address already in use" which is expected
            if !e.to_string().contains("Address already in use") {
                tracing::warn!("Could not start shared registry server: {}", e);
            }
        }
    });
    
    // Note: No need to connect as client since registry server runs in same process
    
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
    
    // Set the PID as environment variable for child processes
    std::env::set_var("VEDA_PID", &app.instance_id.to_string());
    
    // Start IPC server - it will handle Claude session ID routing
    let ipc_tx = app.message_tx.clone();
    
    // Start the IPC server in the background
    tokio::spawn(async move {
        start_ipc_server(ipc_tx).await;
    });
    
    tracing::info!("Started Veda with PID: {}", app.instance_id);

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
    let socket_path = format!("/tmp/veda-{}.sock", app.instance_id);
    let _ = std::fs::remove_file(&socket_path);
    tracing::info!("Cleaned up socket: {}", socket_path);

    Ok(())
}

async fn run_mcp_server() -> Result<()> {
    use std::io::{BufRead, Write};
    
    // Set up simple logging for MCP server mode
    eprintln!("[veda-mcp-server] Starting with session: {}", 
        std::env::var("VEDA_SESSION_ID").unwrap_or_else(|_| "default".to_string())
    );
    
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    
    let stdin = stdin.lock();
    for line in stdin.lines() {
        let line = line?;
        let request: Value = serde_json::from_str(&line)?;
        let response = process_mcp_request(&request).await;
        
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    
    Ok(())
}

async fn process_mcp_request(request: &Value) -> Value {
    match request["method"].as_str() {
        Some("tools/list") => create_tools_list_response(&request["id"]),
        Some("tools/call") => {
            let tool_name = request["params"]["name"].as_str().unwrap_or("");
            let tool_input = &request["params"]["arguments"];
            create_tool_call_response(&request["id"], tool_name, tool_input).await
        }
        Some("initialize") => create_initialize_response(&request["id"]),
        _ => create_error_response(&request["id"]),
    }
}

fn create_tools_list_response(request_id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "tools": [
                {
                    "name": "veda_spawn_instances",
                    "description": "Spawn additional Veda Slices to work on a task in parallel",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "task_description": {
                                "type": "string",
                                "description": "Description of the task that will be divided among instances"
                            },
                            "num_instances": {
                                "type": "number",
                                "description": "Number of additional Veda Slices to spawn (1-3)",
                                "minimum": 1,
                                "maximum": 3
                            }
                        },
                        "required": ["task_description"]
                    }
                },
                {
                    "name": "veda_list_instances",
                    "description": "List all currently active Veda Slices",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "veda_close_instance",
                    "description": "Close a specific Veda Slice by name",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "instance_name": {
                                "type": "string",
                                "description": "Name of the Veda Slice to close (e.g., 'Slice 2')"
                            }
                        },
                        "required": ["instance_name"]
                    }
                }
            ]
        }
    })
}

async fn create_tool_call_response(request_id: &Value, tool_name: &str, tool_input: &Value) -> Value {
    // Get the session ID from environment
    let veda_session = std::env::var("VEDA_SESSION_ID").unwrap_or_else(|_| "default".to_string());
    
    match tool_name {
        "veda_spawn_instances" => {
            // Send message to Veda via shared IPC
            let ipc_message = json!({
                "type": "spawn_instances",
                "session_id": veda_session,
                "task_description": tool_input["task_description"].as_str().unwrap_or(""),
                "num_instances": tool_input["num_instances"].as_u64().unwrap_or(2)
            });
            
            match send_to_veda_via_shared_ipc(&veda_session, &ipc_message).await {
                Ok(response) => {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": {
                            "content": [
                                {
                                    "type": "text",
                                    "text": response
                                }
                            ]
                        }
                    })
                }
                Err(e) => {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": {
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("⚠️ Could not connect to Veda: {}. Make sure Veda is running.", e)
                                }
                            ]
                        }
                    })
                }
            }
        }
        "veda_list_instances" => {
            let ipc_message = json!({
                "type": "list_instances",
                "session_id": veda_session
            });
            
            match send_to_veda_via_shared_ipc(&veda_session, &ipc_message).await {
                Ok(response) => {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": {
                            "content": [
                                {
                                    "type": "text",
                                    "text": response
                                }
                            ]
                        }
                    })
                }
                Err(e) => {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": {
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("⚠️ Could not connect to Veda: {}", e)
                                }
                            ]
                        }
                    })
                }
            }
        }
        "veda_close_instance" => {
            let ipc_message = json!({
                "type": "close_instance",
                "session_id": veda_session,
                "instance_name": tool_input["instance_name"].as_str().unwrap_or("")
            });
            
            match send_to_veda_via_shared_ipc(&veda_session, &ipc_message).await {
                Ok(response) => {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": {
                            "content": [
                                {
                                    "type": "text",
                                    "text": response
                                }
                            ]
                        }
                    })
                }
                Err(e) => {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": {
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("⚠️ Could not connect to Veda: {}", e)
                                }
                            ]
                        }
                    })
                }
            }
        }
        _ => {
            json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {
                    "code": -32601,
                    "message": "Method not found"
                }
            })
        }
    }
}

async fn send_to_veda_via_shared_ipc(session_id: &str, message: &Value) -> Result<String, Box<dyn std::error::Error>> {
    use tokio::net::UnixStream;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    // Use the same socket path as the shared IPC server
    let socket_path = crate::shared_ipc::get_socket_path();
    let mut stream = UnixStream::connect(&socket_path).await?;
    
    // Send message
    let msg_str = serde_json::to_string(message)?;
    stream.write_all(msg_str.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    
    // Read response
    let mut buffer = vec![0; 4096];
    let n = stream.read(&mut buffer).await?;
    let response = String::from_utf8_lossy(&buffer[..n]).to_string();
    
    Ok(response)
}

fn create_initialize_response(request_id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "veda-mcp-server",
                "version": "1.0.0"
            }
        }
    })
}

fn create_error_response(request_id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {
            "code": -32601,
            "message": "Method not found"
        }
    })
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
                    // Handle paste event based on current view
                    if app.show_global_view {
                        // Paste into global textarea
                        if let Some(ref mut global_textarea) = app.global_textarea {
                            tracing::debug!("Paste event detected in global view with {} characters", data.len());
                            for ch in data.chars() {
                                use ratatui::crossterm::event::{Event as RatatuiEvent, KeyEvent, KeyCode as RatatuiKeyCode, KeyModifiers as RatatuiKeyModifiers};
                                let key_event = if ch == '\n' {
                                    KeyEvent::new(RatatuiKeyCode::Enter, RatatuiKeyModifiers::NONE)
                                } else {
                                    KeyEvent::new(RatatuiKeyCode::Char(ch), RatatuiKeyModifiers::NONE)
                                };
                                global_textarea.input(RatatuiEvent::Key(key_event));
                            }
                        }
                    } else if let Some(instance) = app.current_instance_mut() {
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
                            // Handle Enter based on current view
                            if app.show_global_view {
                                // Global view: extract message from global textarea and broadcast
                                if let Some(ref mut global_textarea) = app.global_textarea {
                                    if !global_textarea.is_empty() {
                                        let message = global_textarea.lines().join("\n");
                                        // Clear global textarea
                                        app.global_textarea = Some(TextArea::default());
                                        // Broadcast to all slices
                                        app.broadcast_to_all_slices(message).await;
                                    }
                                }
                            } else {
                                // Regular slice view: existing Enter handling
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
                        }
                        _ => {
                            // Pass all other key events to the appropriate textarea
                            if app.show_global_view {
                                // Input to global textarea
                                if let Some(ref mut global_textarea) = app.global_textarea {
                                    use ratatui::crossterm::event::Event as RatatuiEvent;
                                    global_textarea.input(RatatuiEvent::Key(key));
                                }
                            } else if let Some(instance) = app.current_instance_mut() {
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
                                    // Account for Global view offset when getting instance
                                    let instance_idx = if app.show_global_view && i > 0 {
                                        i - 1  // Adjust for Global tab at index 0
                                    } else {
                                        i
                                    };
                                    
                                    if i == 0 && app.show_global_view {
                                        tracing::info!("Clicked Global tab at ({}, {})", mouse.column, mouse.row);
                                    } else if let Some(instance) = app.instances.get(instance_idx) {
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

    // Header with tabs - prepend "Global" to the list
    let mut titles: Vec<Line> = vec![Line::from("Global")];
    titles.extend(app.instances
        .iter()
        .map(|instance| Line::from(instance.name.clone())));
    
    // Adjust selection - if current_tab is 0, we're on a real slice, so add 1 for the UI
    let ui_selected_tab = if app.show_global_view { 0 } else { app.current_tab + 1 };
    
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Veda Slices "))
        .select(ui_selected_tab)
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
        
        // Add Global tab rect
        let global_width = "Global".len() as u16 + 2; // +2 for padding
        let global_rect = Rect {
            x: current_x,
            y: tab_area.y,
            width: global_width,
            height: 1,
        };
        app.tab_rects.push(global_rect);
        current_x += global_width;
        
        // Add slice tab rects
        for instance in &app.instances {
            let tab_width = instance.name.len() as u16 + 2; // +2 for padding like " Slice 0 "
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
    
    // Determine which messages to show
    let mut all_lines = Vec::new();
    
    if app.show_global_view {
        // Global view: show messages from ALL slices with slice identifiers
        for (slice_idx, instance) in app.instances.iter().enumerate() {
            for (_msg_idx, msg) in instance.messages.iter().enumerate() {
                // Add slice identifier prefix
                let mut content = vec![
                    Span::styled(format!("[Slice {}] ", slice_idx), Style::default().fg(Color::Magenta)),
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
                            "DeepSeek" | "Ollama" => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                            _ => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                        },
                    ),
                    Span::raw(": "),
                ];
                
                // Handle special message types
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
                    // Sanitize and add content
                    let safe_content = msg.content
                        .chars()
                        .map(|c| if c.is_control() && c != '\n' && c != '\t' { '?' } else { c })
                        .collect::<String>();
                    content.push(Span::raw(safe_content));
                }
                
                // Create and add the line
                all_lines.push(Line::from(content));
                all_lines.push(Line::from("")); // Empty line for readability
            }
        }
        
        // Create the messages paragraph for global view (full area)
        let messages_paragraph = Paragraph::new(all_lines)
            .block(Block::default().borders(Borders::ALL).title(format!(
                "Global View - All Slices [Auto: {}] [CoT: {}] [Coord: {}]",
                if app.auto_mode { "ON" } else { "OFF" },
                if app.show_chain_of_thought { "ON" } else { "OFF" },
                if app.coordination_enabled { "ON" } else { "OFF" },
            )))
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false })
            .scroll((0, 0)); // TODO: Add global scroll offset
        f.render_widget(messages_paragraph, chunks[1]);

        // Overlay slice status pane if we have multiple slices (top-right corner)
        if app.instances.len() > 1 {
            let messages_area = chunks[1];
            
            // Calculate overlay size (max 1/5 width, dynamic height based on slice count)
            let status_width = (messages_area.width / 5).max(20).min(35);
            let status_height = (app.instances.len() as u16 * 3 + 2).min(messages_area.height.saturating_sub(2));
            
            // Position in top-right corner of messages area (inside borders)
            let overlay_area = Rect {
                x: messages_area.x + messages_area.width.saturating_sub(status_width + 1),
                y: messages_area.y + 1, // Just inside the top border
                width: status_width,
                height: status_height,
            };

            // Clear the overlay area first
            f.render_widget(Clear, overlay_area);
            
            let mut status_lines = Vec::new();
            
            for (idx, instance) in app.instances.iter().enumerate() {
                // Determine current action based on processing state and recent messages
                let action = if instance.is_processing {
                    "Processing"
                } else {
                    // Look at the last few messages to infer action
                    if let Some(last_msg) = instance.messages.last() {
                        match last_msg.sender.as_str() {
                            "Tool" => {
                                // Parse tool output for action hints
                                if last_msg.content.contains("Edit") || last_msg.content.contains("edit") {
                                    "Editing"
                                } else if last_msg.content.contains("Read") || last_msg.content.contains("read") {
                                    "Reading"
                                } else if last_msg.content.contains("Write") || last_msg.content.contains("write") {
                                    "Writing"
                                } else if last_msg.content.contains("Bash") || last_msg.content.contains("command") {
                                    "Running"
                                } else {
                                    "Tool"
                                }
                            }
                            "Claude" => "Thinking",
                            "You" => "Waiting",
                            _ => "Idle"
                        }
                    } else {
                        "Idle"
                    }
                };

                // Get working context (2 words max)
                let context = if let Some(_session_id) = &instance.session_id {
                    // Try to extract meaningful context from working directory
                    if instance.working_directory.contains("veda") {
                        "veda proj"
                    } else if instance.working_directory.contains("src") {
                        "src code"
                    } else {
                        "gen task"
                    }
                } else {
                    "unstarted"
                };

                // Create compact status line for this slice
                let status_color = if instance.is_processing {
                    Color::Yellow
                } else if instance.messages.is_empty() {
                    Color::DarkGray
                } else {
                    Color::Green
                };

                status_lines.push(Line::from(vec![
                    Span::styled(format!("S{}: ", idx), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(action, Style::default().fg(status_color)),
                ]));
                status_lines.push(Line::from(vec![
                    Span::styled(format!("  {}", context), Style::default().fg(Color::White)),
                ]));
            }

            let status_paragraph = Paragraph::new(status_lines)
                .block(Block::default().borders(Borders::ALL).title("Status"))
                .style(Style::default().fg(Color::White).bg(Color::Black))
                .wrap(Wrap { trim: true });
            f.render_widget(status_paragraph, overlay_area);
        }
        
        // Input area in global view - now supports broadcasting
        // Create a temporary textarea for global view
        if app.global_textarea.is_none() {
            app.global_textarea = Some(TextArea::default());
        }
        
        if let Some(ref mut global_textarea) = app.global_textarea {
            let global_input_title = if app.instances.iter().any(|i| i.is_processing) {
                "Input (Broadcast to ALL slices) [Some slices processing - will interrupt]"
            } else {
                "Input (Broadcast to ALL slices)"
            };
            
            global_textarea.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(global_input_title)
                    .border_style(Style::default().fg(Color::Yellow))
            );
            f.render_widget(global_textarea.widget(), chunks[2]);
        }
        
    } else if let Some(instance) = app.instances.get_mut(app.current_tab) {
        // Regular slice view - existing code
        instance.auto_scroll_with_width(Some(message_area_height), Some(message_area_width));
        
        // Calculate which messages to show based on scroll offset
        let skip_lines = instance.scroll_offset as usize / 2; // Each message takes 2 lines
        let visible_messages = instance.messages.iter().skip(skip_lines);
        
        for (i, msg) in visible_messages.enumerate() {
            let actual_idx = i + skip_lines;
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
                        "DeepSeek" | "Ollama" => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
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
            
            // Apply selection highlighting using actual message index
            let mut style = Style::default();
            if let (Some(start), Some(end)) = (instance.selection_start, instance.selection_end) {
                let line_y = actual_idx as u16;
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
                    all_lines.push(Line::from("")); // Empty line for readability
                }
                Err(e) => {
                    tracing::error!("Failed to render message line {}: {:?}", i, e);
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
            .wrap(Wrap { trim: false });
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

