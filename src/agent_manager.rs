use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    path::PathBuf, // Remove unused Path import
    process::Stdio,
    sync::{atomic::{AtomicU32, Ordering}, Arc},
    time::Duration,
};
use tokio::{
    fs,
    process::{Child, Command},
    sync::{Mutex, Notify}, // Import Notify
    task::JoinHandle,
    time::sleep,
};
use tracing::{debug, error, info, warn};

use crate::constants; // Assuming constants like HANDOFF_DIR are defined there

// Represents the possible states of an agent
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub enum AgentStatus {
    Starting,
    Running,
    Waiting, // e.g., waiting for handoff file
    Handoff, // In the process of handing off
    Finished,
    Failed(String), // Include error message
}

// Holds information about a running agent process
#[derive(Debug)]
pub struct AgentInfo {
    pub id: u32,
    pub role: String,
    pub status: AgentStatus,
    pub process: Option<Child>, // The actual OS process
    pub task_handle: Option<JoinHandle<()>>, // Handle for monitoring task (if any)
    pub output_buffer: Vec<String>, // Store recent output lines
                                    // TODO: Add fields for model, current goal, etc.
}

// Simplified status for UI/API reporting
#[derive(Clone, Debug, serde::Serialize)]
pub struct AgentStatusReport {
    id: u32,
    role: String,
    status: AgentStatus,
    // Add other relevant fields like model, uptime, etc. later
}

const AGENT_OUTPUT_BUFFER_SIZE: usize = 100; // Max lines to keep in buffer

pub struct AgentManager {
    active_agents: Arc<Mutex<HashMap<u32, AgentInfo>>>,
    next_agent_id: AtomicU32, // Simple atomic counter for unique IDs
    handoff_dir: PathBuf,
    // Handle for the main monitoring task
    monitor_task_handle: Mutex<Option<JoinHandle<()>>>,
    // Used to signal the main manager task in main.rs to exit
    shutdown_notify: Arc<Notify>,
}

impl AgentManager {
    pub async fn new() -> Result<Self> {
        info!("Initializing Agent Manager...");
        let handoff_path = PathBuf::from(constants::HANDOFF_DIR.as_str());
        // Ensure handoff directory exists
        fs::create_dir_all(&handoff_path)
            .await
            .context(format!("Failed to create handoff directory: {:?}", handoff_path))?;

        Ok(Self {
            active_agents: Arc::new(Mutex::new(HashMap::new())),
            next_agent_id: AtomicU32::new(1), // Start IDs from 1
            handoff_dir: handoff_path,
            monitor_task_handle: Mutex::new(None),
            shutdown_notify: Arc::new(Notify::new()), // Initialize Notify
        })
    }

    // Generates the next unique agent ID
    fn get_next_id(&self) -> u32 {
        self.next_agent_id.fetch_add(1, Ordering::SeqCst)
    }

    // Main entry point to start the manager and potentially initial agents
    pub async fn start(self: Arc<Self>, initial_prompt: Option<String>) -> Result<()> {
        info!("Starting Agent Manager background tasks...");

        // Spawn the main monitoring loop
        let manager_clone = self.clone();
        let monitor_handle = tokio::spawn(async move {
            manager_clone.monitor_agents_loop().await;
        });
        *self.monitor_task_handle.lock().await = Some(monitor_handle);


        if let Some(prompt) = initial_prompt {
            info!("Initial prompt provided. Spawning coordinator agent...");
            // TODO: Define roles and initial agent logic more formally
            // For now, just spawn a placeholder if prompt exists
            self.spawn_agent("coordinator".to_string(), prompt).await?;
        } else {
            info!("No initial prompt. Waiting for instructions via API/Chat.");
        }

        Ok(())
    }

    // Spawns a new agent process (placeholder for now)
    pub async fn spawn_agent(&self, role: String, _prompt: String) -> Result<u32> {
        let agent_id = self.get_next_id();
        info!("Spawning agent ID: {}, Role: {}", agent_id, role);

        // --- Placeholder Command ---
        // TODO: Replace this with actual Aider command construction
        let mut command = Command::new("sleep");
        command.arg("30"); // Sleep for 30 seconds as a placeholder
        // --- End Placeholder ---

        // Configure stdio for capturing output later
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        // TODO: Configure stdin for sending commands later

        let child = command.spawn().context(format!(
            "Failed to spawn agent process for role '{}'",
            role
        ))?;

        let agent_info = AgentInfo {
            id: agent_id,
            role: role.clone(),
            status: AgentStatus::Running, // Assume running for now
            process: Some(child),
            task_handle: None, // No specific task for this agent yet
            output_buffer: Vec::with_capacity(AGENT_OUTPUT_BUFFER_SIZE),
        };

        // Add agent to the map
        let mut agents = self.active_agents.lock().await;
        agents.insert(agent_id, agent_info);
        info!("Agent {} ({}) added to active list.", agent_id, role);

        Ok(agent_id)
    }

    // The main loop for monitoring agent statuses
    async fn monitor_agents_loop(&self) {
        info!("Agent monitoring loop started.");
        loop {
            sleep(Duration::from_secs(5)).await; // Check every 5 seconds

            let mut agents = self.active_agents.lock().await;
            if agents.is_empty() {
                debug!("No active agents to monitor.");
                continue;
            }

            let mut finished_agent_ids = Vec::new();

            for (id, agent_info) in agents.iter_mut() {
                if agent_info.status == AgentStatus::Finished || matches!(agent_info.status, AgentStatus::Failed(_)) {
                    continue; // Already terminated
                }

                if let Some(process) = agent_info.process.as_mut() {
                    match process.try_wait() {
                        Ok(Some(status)) => {
                            // Process finished
                            if status.success() {
                                info!("Agent {} ({}) finished successfully.", id, agent_info.role);
                                agent_info.status = AgentStatus::Finished;
                            } else {
                                let err_msg = format!("Agent {} ({}) failed with status: {}", id, agent_info.role, status);
                                error!("{}", err_msg);
                                agent_info.status = AgentStatus::Failed(err_msg);
                            }
                            finished_agent_ids.push(*id); // Mark for potential cleanup later if needed
                        }
                        Ok(None) => {
                            // Process still running
                            agent_info.status = AgentStatus::Running; // Ensure status is Running if wait returns None
                            debug!("Agent {} ({}) is still running.", id, agent_info.role);
                            // TODO: Check for handoff files, read output buffer etc.
                        }
                        Err(e) => {
                            let err_msg = format!("Error waiting for agent {} ({}): {}", id, agent_info.role, e);
                            error!("{}", err_msg);
                            agent_info.status = AgentStatus::Failed(err_msg);
                            finished_agent_ids.push(*id);
                        }
                    }
                } else {
                    // No process associated? Should not happen for Running agents
                    warn!("Agent {} ({}) has no process handle but is not marked as finished/failed.", id, agent_info.role);
                    // Optionally mark as failed or investigate
                }
            }

            // Optional: Remove finished/failed agents immediately if desired,
            // or keep them for reporting until explicitly cleared.
            // for id in finished_agent_ids {
            //     agents.remove(&id);
            //     info!("Removed agent {} from active list.", id);
            // }
        }
        // info!("Agent monitoring loop stopped."); // This line might not be reached in normal operation
    }

    // Provides a summary of active agents for UI/API
    pub async fn get_status_report(&self) -> Vec<AgentStatusReport> {
        let agents = self.active_agents.lock().await;
        agents
            .values()
            .map(|info| AgentStatusReport {
                id: info.id,
                role: info.role.clone(),
                status: info.status.clone(),
            })
            .collect()
    }


    // Graceful shutdown: stop monitoring and terminate agents
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping Agent Manager...");

        // Stop the monitoring loop first
        if let Some(handle) = self.monitor_task_handle.lock().await.take() {
            info!("Aborting monitor task...");
            handle.abort();
            let _ = handle.await; // Wait for abort to complete (ignore result)
            info!("Monitor task stopped.");
        } else {
            info!("Monitor task was not running.");
        }


        let mut agents = self.active_agents.lock().await;
        info!("Terminating {} active agent(s)...", agents.len());

        for (id, agent_info) in agents.iter_mut() {
             // Check if process exists and is likely running
            if agent_info.process.is_some() && agent_info.status != AgentStatus::Finished && !matches!(agent_info.status, AgentStatus::Failed(_)) {
                info!("Attempting to terminate agent {} ({})", id, agent_info.role);
                if let Some(process) = agent_info.process.as_mut() {
                    match process.start_kill() { // Send SIGKILL
                        Ok(_) => {
                            info!("Sent kill signal to agent {}", id);
                            // Optionally wait a short time for process to exit
                            // let _ = process.wait().await;
                            agent_info.status = AgentStatus::Failed("Terminated by shutdown".to_string());
                        }
                        Err(e) => {
                            error!("Failed to kill agent {} process: {}", id, e);
                             agent_info.status = AgentStatus::Failed(format!("Failed to terminate: {}", e));
                        }
                    }
                }
            }
             // Abort any associated task handles
             if let Some(task) = agent_info.task_handle.take() {
                 task.abort();
             }
        }
        // Clear the map after attempting termination
        // agents.clear(); // Or keep terminated agents for final status reporting? Let's keep them for now.
        info!("Agent termination process complete.");

        // Signal the main manager task to exit
        self.shutdown_notify.notify_waiters();
        info!("Shutdown notification sent.");

        Ok(())
    }
}
