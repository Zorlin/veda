use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc; // Import Arc
use tracing::{error, info};

// Declare modules that will be part of the project
mod agent_manager;
mod chat;
mod constants;
mod web_server;

// Define the command-line interface structure using clap
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Define the available subcommands
#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Start the Veda background service and web server.
    Start {
        #[arg(long, help = "Optional initial goal prompt for the agents.")]
        prompt: Option<String>,
        #[arg(long, default_value_t = 9900, help = "Port for the web server.")]
        port: u16,
    },
    /// Engage in a text-based chat session with Veda.
    Chat,
    /// Set configuration values.
    Set {
        #[command(subcommand)]
        target: SetCommands,
    },
    /// Stop the Veda service.
    Stop,
}

// Define subcommands for the 'set' command
#[derive(clap::Subcommand, Debug)]
enum SetCommands {
    /// Manually override the number of Aider instances Veda manages ('auto' is recommended).
    Instances {
        #[arg(value_parser = clap::value_parser!(String))] // Accept "auto" or a number string
        value: String,
    },
    // Add other potential 'set' subcommands here
}

// The main entry point of the application, using tokio's async runtime
#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present (for environment variables like API keys)
    dotenvy::dotenv().ok();

    // Initialize tracing (logging) subscriber
    // Reads log level from RUST_LOG environment variable (e.g., RUST_LOG=info,veda=debug)
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Parse command-line arguments
    let cli = Cli::parse();

    info!("Veda starting with command: {:?}", cli.command);

    // Handle the parsed command
    match cli.command {
        Commands::Start { prompt, port } => { // Use prompt again
            info!("Starting Veda services on port {}...", port);

            // Initialize AgentManager
            let agent_manager = Arc::new(
                agent_manager::AgentManager::new()
                    .await
                    .context("Failed to initialize Agent Manager")?,
            );

            // Clone Arc for web server state
            let agent_manager_web_clone = agent_manager.clone();
            // Start the web server in a separate asynchronous task
            // Declare as mutable for use in tokio::select!
            let mut web_server_handle = tokio::spawn(async move {
                // Pass agent manager handle to web server
                if let Err(e) = web_server::start_web_server(port, agent_manager_web_clone).await {
                    error!("Web server failed: {:?}", e);
                }
            });

            // Clone Arc for agent manager task
            let agent_manager_task_clone = agent_manager.clone();
            // Spawn the agent manager's main loop task
            let mut agent_manager_handle = tokio::spawn(async move {
                 if let Err(e) = agent_manager_task_clone.start(prompt).await {
                     error!("Agent manager task failed: {:?}", e);
                 }
             });

            // Keep the main thread alive and wait for shutdown signals or task completion
            let mut web_server_handle = tokio::spawn(async move {
                if let Err(e) = web_server::start_web_server(port).await {
                    error!("Web server failed: {:?}", e);
                }
            });

            // TODO: Start AgentManager loop if needed, potentially passing the prompt
            // let agent_manager_clone = agent_manager.clone();
            // let agent_manager_handle = tokio::spawn(async move {
            //     if let Some(p) = prompt {
            //         info!("Passing initial prompt to Agent Manager: {}", p);
            //     }
            //     if let Err(e) = agent_manager_clone.start(prompt).await {
            //         error!("Agent manager failed: {:?}", e);
            //     }
            // });

            // Keep the main thread alive and wait for shutdown signals or task completion
            let ctrl_c = tokio::signal::ctrl_c();
            // Pin the ctrl_c future to the stack so its address is stable
            tokio::pin!(ctrl_c);

            tokio::select! {
                // Wait for Ctrl-C signal for graceful shutdown
                _ = &mut ctrl_c => {
                    info!("Ctrl-C received, initiating shutdown...");
                    // Initiate graceful shutdown for AgentManager
                    if let Err(e) = agent_manager.stop().await {
                        error!("Error stopping agent manager: {:?}", e);
                    }
                }
                // Handle potential completion/failure of the web server task
                res = &mut web_server_handle => {
                     match res {
                         Ok(_) => info!("Web server task completed unexpectedly."),
                         // Handle JoinError (e.g., if the task panicked)
                         Err(e) if e.is_panic() => error!("Web server task panicked: {:?}", e),
                         Err(e) => error!("Web server task failed: {:?}", e),
                     }
                }
                 // Handle potential completion/failure of the agent manager task
                 res = &mut agent_manager_handle => {
                    match res {
                        Ok(_) => info!("Agent manager task completed unexpectedly."),
                        Err(e) if e.is_panic() => error!("Agent manager task panicked: {:?}", e),
                        Err(e) => error!("Agent manager task failed: {:?}", e),
                    }
                 }
            }

            // After select! finishes (due to Ctrl+C or task completion), ensure shutdown.
            info!("Shutting down remaining tasks...");
            if !web_server_handle.is_finished() {
                 info!("Aborting web server task...");
                 web_server_handle.abort();
            }
             if !agent_manager_handle.is_finished() {
                 info!("Aborting agent manager task...");
                 agent_manager_handle.abort();
                 // Also ensure agent_manager.stop() is called if the manager task finished early
                 if let Err(e) = agent_manager.stop().await {
                     error!("Error during final agent manager stop: {:?}", e);
                 }
             }
            info!("Shutdown complete.");
        }
        Commands::Chat => {
            info!("Starting interactive chat session...");
            // Call the placeholder chat function
            chat::run_readiness_chat()
                .await
                .context("Chat session failed")?;
            info!("Chat session finished.");
        }
        Commands::Set { target } => match target {
            SetCommands::Instances { value } => {
                info!("Setting instances to: {}", value);
                // TODO: Implement logic to communicate this setting to a running service or store it
                println!(
                    "Set instances to {} (placeholder). Implement setting logic.",
                    value
                );
            }
        },
        Commands::Stop => {
            info!("Stopping Veda services...");
            // TODO: Implement logic to gracefully stop running services (e.g., send signal via IPC)
            println!("Stopping Veda (placeholder). Implement service shutdown.");
        }
    }

    Ok(())
}
