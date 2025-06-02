use chrono::{Local, DateTime};
use uuid::Uuid;
use tui_textarea::TextArea;
use crate::deepseek::DeepSeekMessage;
use crate::ui_components::ScrollableTextArea;

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub tool_name: String,
    pub parameters: String,
    pub result: Option<String>,
    pub status: ToolCallStatus,
}

#[derive(Debug, Clone)]
pub enum ToolCallStatus {
    Requested,
    InProgress,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Message {
    pub timestamp: String,
    pub sender: String,
    pub content: String,
    // For DeepSeek messages
    pub is_thinking: bool,
    pub is_collapsed: bool,
    // System-generated message (not from actual Claude output)
    pub is_system_generated: bool,
    // Tool usage tracking
    pub tool_calls: Vec<ToolCall>,
    pub is_tool_use: bool,
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug)]
pub struct TodoListState {
    pub items: Vec<TodoItem>,
    pub visible: bool,
    pub last_update: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub enum BackgroundTask {
    ContinuousTesting,
    CodeQualityChecks,
    PerformanceProfiling,
    SecurityScanning,
    DependencyUpdates,
    DocumentationGeneration,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SliceState {
    Available,           // Ready for new tasks
    WorkingOnTask,      // Currently working on a user task
    SpawningInstances,  // Spawning other instances for parallel work
    BackgroundWork,     // Performing background maintenance tasks
}

pub struct ClaudeInstance {
    pub id: Uuid,
    pub name: String,
    pub messages: Vec<Message>,
    pub textarea: TextArea<'static>,
    pub is_processing: bool,
    // Text selection state
    pub selection_start: Option<(u16, u16)>,
    pub selection_end: Option<(u16, u16)>,
    pub selecting: bool,
    pub scroll_offset: u16,
    // Track tool use attempts
    pub last_tool_attempts: Vec<String>,
    // Track successful tool usage to avoid unnecessary permission checks
    pub successful_tools: Vec<String>,
    // Track tools that have been approved after permission denial
    pub approved_tools: Vec<String>,
    // Claude session ID for resume
    pub session_id: Option<String>,
    // DeepSeek streaming state
    pub deepseek_streaming: bool,
    pub deepseek_receiver: Option<tokio::sync::mpsc::Receiver<DeepSeekMessage>>,
    pub deepseek_accumulated_text: String,
    // Last activity for stall detection
    pub last_activity: DateTime<Local>,
    // Stall detection state
    pub stall_delay_seconds: i64,
    pub stall_check_sent: bool,
    pub stall_intervention_in_progress: bool,
    pub last_stall_check: Option<DateTime<Local>>,
    // Slice management
    pub state: SliceState,
    pub background_task: Option<BackgroundTask>,
    pub assigned_task: Option<String>,
    pub task_description: Option<String>,
    pub work_assignment_received: bool,
    // Working directory
    pub working_directory: String,
    // Scrollable text area for messages
    pub scrollable_messages: ScrollableTextArea,
}

pub struct GlobalState {
    pub auto_mode: bool,
    pub coordination_mode: bool,
    pub global_context: String,
    pub last_coordination_update: DateTime<Local>,
}

pub struct AppState {
    pub instances: Vec<ClaudeInstance>,
    pub current_tab: usize,
    pub global_state: GlobalState,
    pub todo_state: TodoListState,
    pub show_help: bool,
    pub help_scroll: u16,
    pub debug_mode: bool,
    pub debug_log: Vec<String>,
}

impl Message {
    pub fn new(sender: String, content: String) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            sender,
            content,
            is_thinking: false,
            is_collapsed: false,
            is_system_generated: false,
            tool_calls: Vec::new(),
            is_tool_use: false,
        }
    }

    pub fn system(content: String) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            sender: "System".to_string(),
            content,
            is_thinking: false,
            is_collapsed: false,
            is_system_generated: true,
            tool_calls: Vec::new(),
            is_tool_use: false,
        }
    }

    pub fn deepseek(content: String, is_thinking: bool) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            sender: "DeepSeek".to_string(),
            content,
            is_thinking,
            is_collapsed: false,
            is_system_generated: false,
            tool_calls: Vec::new(),
            is_tool_use: false,
        }
    }

    pub fn tool_use(sender: String, tool_name: String, parameters: String) -> Self {
        let tool_call = ToolCall {
            tool_name: tool_name.clone(),
            parameters: parameters.clone(),
            result: None,
            status: ToolCallStatus::Requested,
        };
        
        let content = format!("🔧 Using tool: {} with parameters: {}", tool_name, parameters);
        
        Self {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            sender,
            content,
            is_thinking: false,
            is_collapsed: false,
            is_system_generated: false,
            tool_calls: vec![tool_call],
            is_tool_use: true,
        }
    }

    pub fn add_tool_result(&mut self, tool_name: &str, result: String, success: bool) {
        if let Some(tool_call) = self.tool_calls.iter_mut().find(|tc| tc.tool_name == tool_name) {
            tool_call.result = Some(result.clone());
            tool_call.status = if success {
                ToolCallStatus::Completed
            } else {
                ToolCallStatus::Failed(result)
            };
            
            // Update content to show result
            let status_icon = if success { "✅" } else { "❌" };
            self.content = format!("{} {} Tool {} result: {}", 
                self.content, status_icon, tool_name, 
                if result.len() > 100 { format!("{}...", &result[..100]) } else { result }
            );
        }
    }
}

impl ClaudeInstance {
    pub fn new(name: String) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text("Type your message to Claude...");
        
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
            deepseek_streaming: false,
            deepseek_receiver: None,
            deepseek_accumulated_text: String::new(),
            last_activity: Local::now(),
            stall_delay_seconds: 10,
            stall_check_sent: false,
            stall_intervention_in_progress: false,
            last_stall_check: None,
            state: SliceState::Available,
            background_task: None,
            assigned_task: None,
            task_description: None,
            work_assignment_received: false,
            working_directory: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            scrollable_messages: ScrollableTextArea::new(),
        }
    }

    pub fn should_check_for_stall(&self) -> bool {
        if self.is_processing || self.stall_check_sent || self.stall_intervention_in_progress {
            tracing::debug!("Stall check blocked: processing={}, check_sent={}, intervention={}", 
                self.is_processing, self.stall_check_sent, self.stall_intervention_in_progress);
            return false;
        }
        
        // CRITICAL: Only check for stalls when Claude has finished responding and is waiting for user input
        let claude_has_responded = self.messages.iter().any(|m| m.sender == "Claude");
        if !claude_has_responded {
            tracing::debug!("Stall check blocked: Claude hasn't responded yet");
            return false;
        }
        
        // Don't trigger if user hasn't sent any messages yet
        let user_has_messages = self.messages.iter().any(|m| m.sender == "You");
        if !user_has_messages {
            tracing::debug!("Stall check blocked: no user messages yet");
            return false;
        }
        
        // Check if enough time has passed since last stall check
        if let Some(last_check) = self.last_stall_check {
            let time_since_last_check = Local::now().signed_duration_since(last_check).num_seconds();
            if time_since_last_check < 5 { // Minimum 5 seconds between checks
                tracing::debug!("Stall check blocked: only {} seconds since last check", time_since_last_check);
                return false;
            }
        }

        let elapsed = Local::now().signed_duration_since(self.last_activity).num_seconds();
        let should_stall = elapsed > self.stall_delay_seconds;
        
        if should_stall {
            tracing::info!("Stall condition met: {} seconds elapsed (threshold: {})", 
                elapsed, self.stall_delay_seconds);
        } else {
            tracing::debug!("No stall: {} seconds elapsed (threshold: {})", 
                elapsed, self.stall_delay_seconds);
        }

        should_stall
    }

    pub fn add_message(&mut self, message: Message) {
        // Format message for scrollable display
        let formatted_message = format!("[{}] {}: {}", 
            message.timestamp, message.sender, message.content);
        
        // Add to both storage systems
        self.messages.push(message);
        self.scrollable_messages.add_message(formatted_message);
    }

    pub fn sync_scrollable_messages(&mut self) {
        // Clear and rebuild scrollable messages from current message list
        self.scrollable_messages = ScrollableTextArea::new();
        for msg in &self.messages {
            let formatted_message = format!("[{}] {}: {}", 
                msg.timestamp, msg.sender, msg.content);
            self.scrollable_messages.add_message(formatted_message);
        }
    }
}

impl Default for TodoListState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            visible: false,
            last_update: Local::now(),
        }
    }
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            auto_mode: false,
            coordination_mode: false,
            global_context: String::new(),
            last_coordination_update: Local::now(),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        let mut instances = Vec::new();
        instances.push(ClaudeInstance::new("Slice 0".to_string()));

        Self {
            instances,
            current_tab: 0,
            global_state: GlobalState::default(),
            todo_state: TodoListState::default(),
            show_help: false,
            help_scroll: 0,
            debug_mode: false,
            debug_log: Vec::new(),
        }
    }

    pub fn current_instance(&self) -> &ClaudeInstance {
        &self.instances[self.current_tab]
    }

    pub fn current_instance_mut(&mut self) -> &mut ClaudeInstance {
        &mut self.instances[self.current_tab]
    }

    pub fn add_instance(&mut self, name: String) -> usize {
        self.instances.push(ClaudeInstance::new(name));
        self.instances.len() - 1
    }

    pub fn remove_instance(&mut self, index: usize) {
        if self.instances.len() > 1 && index < self.instances.len() {
            self.instances.remove(index);
            if self.current_tab >= self.instances.len() {
                self.current_tab = self.instances.len() - 1;
            }
        }
    }

    pub fn next_tab(&mut self) {
        self.current_tab = (self.current_tab + 1) % self.instances.len();
    }

    pub fn previous_tab(&mut self) {
        if self.current_tab == 0 {
            self.current_tab = self.instances.len() - 1;
        } else {
            self.current_tab -= 1;
        }
    }
}