use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::{UnixListener, UnixStream};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde::{Deserialize, Serialize};
use anyhow::Result;
use tracing::{info, error, warn};

/// Get the appropriate socket path for the current OS
fn get_socket_path() -> String {
    #[cfg(target_os = "macos")]
    {
        // On macOS, use /tmp which is standard and accessible
        "/tmp/veda-shared-registry.sock".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        // On Linux, check if /run/user exists (systemd user session), otherwise use /tmp
        let uid = unsafe { libc::getuid() };
        let user_runtime_dir = format!("/run/user/{}", uid);
        if std::path::Path::new(&user_runtime_dir).exists() {
            format!("{}/veda-shared-registry.sock", user_runtime_dir)
        } else {
            "/tmp/veda-shared-registry.sock".to_string()
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        // Fallback for other Unix-like systems
        "/tmp/veda-shared-registry.sock".to_string()
    }
}

/// Shared state for tracking instances across multiple Veda processes
#[derive(Default, Clone)]
pub struct SharedInstanceRegistry {
    /// Map of Session ID -> number of child instances
    session_instances: Arc<RwLock<HashMap<String, u32>>>,
    /// Map of Session ID -> Veda PID for cross-process coordination  
    session_to_pid: Arc<RwLock<HashMap<String, u32>>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegistryCommand {
    pub command: String,
    pub session_id: String,
    pub value: Option<u32>,
}

/// Inter-Veda coordination message for cross-codebase communication
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VedaCoordinationMessage {
    pub from: String,           // Source repository/Veda instance name
    pub to: Option<String>,     // Target repository (None = broadcast)
    pub message_type: String,   // "RequestChange", "Acknowledge", "Question", "TaskUpdate", etc.
    pub summary: String,        // Brief description of the request/response
    pub content: String,        // Detailed content
    pub task_id: Option<String>, // TaskMaster AI task ID if applicable
    pub timestamp: u64,         // Unix timestamp
    pub reply_to: Option<String>, // Message ID this is replying to
    pub session_context: Option<String>, // Claude session context if needed
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegistryResponse {
    pub success: bool,
    pub message: String,
    pub data: Option<HashMap<String, u32>>,
}

impl SharedInstanceRegistry {
    pub fn new() -> Self {
        Self {
            session_instances: Arc::new(RwLock::new(HashMap::new())),
            session_to_pid: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn increment_instances(&self, session_id: &str, count: u32) -> u32 {
        let mut registry = self.session_instances.write().await;
        let current = registry.entry(session_id.to_string()).or_insert(0);
        *current += count;
        *current
    }

    pub async fn decrement_instances(&self, session_id: &str, count: u32) -> u32 {
        let mut registry = self.session_instances.write().await;
        if let Some(current) = registry.get_mut(session_id) {
            *current = current.saturating_sub(count);
            if *current == 0 {
                registry.remove(session_id);
                0
            } else {
                *current
            }
        } else {
            0
        }
    }

    pub async fn get_instances(&self, session_id: &str) -> u32 {
        let registry = self.session_instances.read().await;
        registry.get(session_id).copied().unwrap_or(0)
    }

    pub async fn get_all_sessions(&self) -> HashMap<String, u32> {
        let registry = self.session_instances.read().await;
        registry.clone()
    }

    pub async fn clear_session(&self, session_id: &str) {
        let mut registry = self.session_instances.write().await;
        registry.remove(session_id);
    }
    
    /// Register sessionID -> Veda PID mapping
    pub async fn register_session_pid(&self, session_id: &str, veda_pid: u32) {
        let mut registry = self.session_to_pid.write().await;
        registry.insert(session_id.to_string(), veda_pid);
    }
    
    /// Get Veda PID for a session
    pub async fn get_session_pid(&self, session_id: &str) -> Option<u32> {
        let registry = self.session_to_pid.read().await;
        registry.get(session_id).copied()
    }
    
    /// Remove sessionID -> PID mapping
    pub async fn unregister_session_pid(&self, session_id: &str) {
        let mut registry = self.session_to_pid.write().await;
        registry.remove(session_id);
    }
    
    /// Get all sessionID -> PID mappings
    pub async fn get_all_session_pids(&self) -> HashMap<String, u32> {
        let registry = self.session_to_pid.read().await;
        registry.clone()
    }
}

/// Start the shared IPC server that multiple Veda instances can connect to
pub async fn start_shared_ipc_server() -> Result<()> {
    let socket_path = get_socket_path();
    
    // Remove existing socket if it exists
    let _ = std::fs::remove_file(&socket_path);
    
    let listener = UnixListener::bind(&socket_path)?;
    info!("Shared IPC registry server listening on {}", socket_path);
    
    let registry = SharedInstanceRegistry::new();
    
    loop {
        match listener.accept().await {
            Ok((socket, _)) => {
                let registry = registry.clone();
                tokio::spawn(handle_registry_connection(socket, registry));
            }
            Err(e) => {
                error!("Failed to accept registry connection: {}", e);
            }
        }
    }
}

async fn handle_registry_connection(
    mut socket: UnixStream,
    registry: SharedInstanceRegistry,
) {
    let (reader, mut writer) = socket.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    
    while reader.read_line(&mut line).await.is_ok() {
        if line.is_empty() {
            break;
        }
        
        match serde_json::from_str::<RegistryCommand>(&line) {
            Ok(cmd) => {
                let response = match cmd.command.as_str() {
                    "increment" => {
                        let count = cmd.value.unwrap_or(1);
                        let total = registry.increment_instances(&cmd.session_id, count).await;
                        RegistryResponse {
                            success: true,
                            message: format!("Incremented to {} instances", total),
                            data: None,
                        }
                    }
                    "decrement" => {
                        let count = cmd.value.unwrap_or(1);
                        let remaining = registry.decrement_instances(&cmd.session_id, count).await;
                        RegistryResponse {
                            success: true,
                            message: format!("{} instances remaining", remaining),
                            data: None,
                        }
                    }
                    "get" => {
                        let count = registry.get_instances(&cmd.session_id).await;
                        RegistryResponse {
                            success: true,
                            message: format!("{} instances for session", count),
                            data: Some(HashMap::from([(cmd.session_id.clone(), count)])),
                        }
                    }
                    "list" => {
                        let all_sessions = registry.get_all_sessions().await;
                        RegistryResponse {
                            success: true,
                            message: format!("{} active sessions", all_sessions.len()),
                            data: Some(all_sessions),
                        }
                    }
                    "clear" => {
                        registry.clear_session(&cmd.session_id).await;
                        RegistryResponse {
                            success: true,
                            message: "Session cleared".to_string(),
                            data: None,
                        }
                    }
                    "register_pid" => {
                        let veda_pid = cmd.value.unwrap_or(0);
                        registry.register_session_pid(&cmd.session_id, veda_pid).await;
                        RegistryResponse {
                            success: true,
                            message: format!("Registered session {} -> PID {}", cmd.session_id, veda_pid),
                            data: None,
                        }
                    }
                    "get_pid" => {
                        if let Some(veda_pid) = registry.get_session_pid(&cmd.session_id).await {
                            RegistryResponse {
                                success: true,
                                message: format!("Session {} -> PID {}", cmd.session_id, veda_pid),
                                data: Some(HashMap::from([(cmd.session_id.clone(), veda_pid)])),
                            }
                        } else {
                            RegistryResponse {
                                success: false,
                                message: format!("No PID found for session {}", cmd.session_id),
                                data: None,
                            }
                        }
                    }
                    "unregister_pid" => {
                        registry.unregister_session_pid(&cmd.session_id).await;
                        RegistryResponse {
                            success: true,
                            message: format!("Unregistered session {}", cmd.session_id),
                            data: None,
                        }
                    }
                    "list_pids" => {
                        let all_pids = registry.get_all_session_pids().await;
                        RegistryResponse {
                            success: true,
                            message: format!("{} session PIDs", all_pids.len()),
                            data: Some(all_pids),
                        }
                    }
                    _ => RegistryResponse {
                        success: false,
                        message: format!("Unknown command: {}", cmd.command),
                        data: None,
                    }
                };
                
                if let Ok(response_json) = serde_json::to_string(&response) {
                    let _ = writer.write_all(response_json.as_bytes()).await;
                    let _ = writer.write_all(b"\n").await;
                }
            }
            Err(e) => {
                warn!("Failed to parse registry command: {}", e);
            }
        }
        
        line.clear();
    }
}

/// Client for connecting to the shared registry
pub struct RegistryClient;

impl RegistryClient {
    pub async fn send_command(command: RegistryCommand) -> Result<RegistryResponse> {
        let socket_path = get_socket_path();
        let mut socket = UnixStream::connect(&socket_path).await?;
        
        let command_json = serde_json::to_string(&command)?;
        socket.write_all(command_json.as_bytes()).await?;
        socket.write_all(b"\n").await?;
        
        let (reader, _) = socket.split();
        let mut reader = BufReader::new(reader);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await?;
        
        let response: RegistryResponse = serde_json::from_str(&response_line)?;
        Ok(response)
    }
    
    pub async fn increment_instances(session_id: &str, count: u32) -> Result<u32> {
        let cmd = RegistryCommand {
            command: "increment".to_string(),
            session_id: session_id.to_string(),
            value: Some(count),
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            // Extract count from message
            if let Some(total_str) = response.message.split(' ').nth(2) {
                if let Ok(total) = total_str.parse::<u32>() {
                    return Ok(total);
                }
            }
        }
        Ok(0)
    }
    
    pub async fn decrement_instances(session_id: &str, count: u32) -> Result<u32> {
        let cmd = RegistryCommand {
            command: "decrement".to_string(),
            session_id: session_id.to_string(),
            value: Some(count),
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            // Extract count from message
            if let Some(remaining_str) = response.message.split(' ').next() {
                if let Ok(remaining) = remaining_str.parse::<u32>() {
                    return Ok(remaining);
                }
            }
        }
        Ok(0)
    }
    
    pub async fn get_instances(session_id: &str) -> Result<u32> {
        let cmd = RegistryCommand {
            command: "get".to_string(),
            session_id: session_id.to_string(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            if let Some(data) = response.data {
                return Ok(data.get(session_id).copied().unwrap_or(0));
            }
        }
        Ok(0)
    }
    
    pub async fn list_all_sessions() -> Result<HashMap<String, u32>> {
        let cmd = RegistryCommand {
            command: "list".to_string(),
            session_id: String::new(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            return Ok(response.data.unwrap_or_default());
        }
        Ok(HashMap::new())
    }
    
    /// Register sessionID -> Veda PID mapping in shared registry
    pub async fn register_session_pid(session_id: &str, veda_pid: u32) -> Result<()> {
        let cmd = RegistryCommand {
            command: "register_pid".to_string(),
            session_id: session_id.to_string(),
            value: Some(veda_pid),
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            info!("✅ Registered session {} -> PID {} in shared registry", session_id, veda_pid);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to register session PID: {}", response.message))
        }
    }
    
    /// Get Veda PID for a session from shared registry
    pub async fn get_session_pid(session_id: &str) -> Result<Option<u32>> {
        let cmd = RegistryCommand {
            command: "get_pid".to_string(),
            session_id: session_id.to_string(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            if let Some(data) = response.data {
                return Ok(data.get(session_id).copied());
            }
        }
        Ok(None)
    }
    
    /// Remove sessionID -> PID mapping from shared registry
    pub async fn unregister_session_pid(session_id: &str) -> Result<()> {
        let cmd = RegistryCommand {
            command: "unregister_pid".to_string(),
            session_id: session_id.to_string(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            info!("✅ Unregistered session {} from shared registry", session_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to unregister session PID: {}", response.message))
        }
    }
    
    /// Get all sessionID -> PID mappings from shared registry
    pub async fn list_all_session_pids() -> Result<HashMap<String, u32>> {
        let cmd = RegistryCommand {
            command: "list_pids".to_string(),
            session_id: String::new(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            return Ok(response.data.unwrap_or_default());
        }
        Ok(HashMap::new())
    }
}