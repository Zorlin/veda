use anyhow::{Context, Result};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router, serve, // Use axum::serve instead of axum::Server
};
use futures::{sink::SinkExt, stream::StreamExt};
use minijinja::{path_loader, Environment};
use minijinja_autoreload::AutoReloader;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::broadcast;
use tower_http::{services::ServeDir, trace::TraceLayer};
use serde::{Deserialize, Serialize}; // Add Serialize/Deserialize
use tracing::{debug, error, info, instrument, warn}; // Added instrument

use crate::agent_manager::{AgentManager, AgentStatusReport}; // Import AgentManager
use crate::llm_interaction; // Import the new module

// Define the structure for messages broadcasted to clients
// Using serde allows easy conversion to/from JSON for WebSocket messages
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct BroadcastMessage {
    // TODO: Define message types (e.g., Chat, StatusUpdate, Error)
    message_type: String,
    payload: serde_json::Value,
}

// Shared application state
#[derive(Clone)]
struct AppState {
    templates: Arc<AutoReloader>,
    // Channel for broadcasting messages to all connected WebSocket clients
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    // Handle to the Agent Manager
    agent_manager: Arc<AgentManager>,
}

// Minijinja Environment setup
fn create_minijinja_env() -> Result<AutoReloader> {
    // Use AutoReloader for development convenience
    let reloader = AutoReloader::new(|notifier| {
        // Create the loader *inside* the closure
        let loader = path_loader("templates");
        let mut env = Environment::new();
        env.set_loader(loader);
        // Watch the templates directory for changes
        notifier.watch_path("templates", true);
        Ok(env)
    });
    Ok(reloader)
}

async fn index_handler(
    State(state): State<AppState>,
) -> Result<axum::response::Html<String>, axum::response::Html<String>> {
    // Acquire env, get template, and render within the same block
    state.templates.acquire_env().and_then(|env| {
        env.get_template("index.html").and_then(|tmpl| {
            let context = minijinja::context! {
                title => "Veda Web UI",
                // Add more context variables as needed
            };
            tmpl.render(context)
        })
    })
    .map(axum::response::Html) // Wrap successful render in Html()
    .map_err(|e| {
        // Handle errors from acquire_env, get_template, or render
        error!("Failed to get or render template: {}", e);
        axum::response::Html(format!("Internal Server Error: {}", e))
    })
}

// API handler to get agent statuses
async fn api_agent_status_handler(
    State(state): State<AppState>,
) -> axum::response::Json<Vec<AgentStatusReport>> {
    debug!("Handling request for /api/agents/status");
    let report = state.agent_manager.get_status_report().await;
    axum::response::Json(report)
}

// Request structure for the synthesis endpoint
#[derive(Deserialize, Debug)]
struct SynthesizeGoalRequest {
    tags: Vec<String>,
}

// Response structure for the synthesis endpoint
#[derive(Serialize)]
struct SynthesizeGoalResponse {
    goal: String,
}

// API handler for synthesizing goals
#[instrument(skip(state, request))]
async fn synthesize_goal_handler(
    State(_state): State<AppState>, // Prefix unused state with underscore
    axum::extract::Json(request): axum::extract::Json<SynthesizeGoalRequest>,
) -> Result<axum::response::Json<SynthesizeGoalResponse>, axum::http::StatusCode> {
    info!(tags = ?request.tags, "Handling request for /api/synthesize-goal");

    if request.tags.is_empty() {
        warn!("Received empty tag list for synthesis.");
        // Return bad request or an empty goal? Let's return empty goal for now.
        return Ok(axum::response::Json(SynthesizeGoalResponse { goal: "".to_string() }));
    }

    match llm_interaction::synthesize_goal_with_ollama(request.tags).await {
        Ok(synthesized_goal) => {
            info!(synthesized_goal = ?synthesized_goal, "Goal synthesized successfully");
            Ok(axum::response::Json(SynthesizeGoalResponse { goal: synthesized_goal }))
        }
        Err(e) => {
            error!("Failed to synthesize goal: {:?}", e);
            // Return an internal server error status code
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}


// WebSocket upgrade handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("WebSocket connection upgrade requested");
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

// Handle individual WebSocket connections
async fn handle_socket(mut socket: WebSocket, state: AppState) {
    info!("New WebSocket connection established");
    let mut broadcast_rx = state.broadcast_tx.subscribe();

    // Send a welcome message or initial state if needed
    let welcome_msg = BroadcastMessage {
        message_type: "Info".to_string(),
        payload: serde_json::json!({"message": "Connected to Veda WebSocket"}),
    };
    if let Ok(json_msg) = serde_json::to_string(&welcome_msg) {
        if socket.send(Message::Text(json_msg)).await.is_err() {
            warn!("Failed to send welcome message to new WebSocket client");
            return; // Close connection if send fails
        }
    }

    // Loop for handling messages from this specific client and broadcasts
    loop {
        tokio::select! {
            // Message received from the broadcast channel
            Ok(msg) = broadcast_rx.recv() => {
                if let Ok(json_msg) = serde_json::to_string(&msg) {
                    // Send the broadcast message to the client
                    if socket.send(Message::Text(json_msg)).await.is_err() {
                        // Client disconnected or error sending
                        warn!("WebSocket client disconnected or send error. Closing connection.");
                        break;
                    }
                } else {
                    error!("Failed to serialize broadcast message");
                }
            }

            // Message received from the client
            Some(Ok(msg)) = socket.recv() => {
                match msg {
                    Message::Text(text) => {
                        info!("Received text message from client: {}", text);
                        // TODO: Process client message (e.g., parse as JSON, handle chat input)
                        // Example: Echo back or broadcast
                        let _response = BroadcastMessage { // Prefixed with underscore
                            message_type: "ChatEcho".to_string(), // Example type
                            payload: serde_json::json!({ "original": text }),
                        };
                        // Example: Broadcast received message (or handle differently)
                        // Let's just log for now, actual handling depends on message content
                        // if state.broadcast_tx.send(response).is_err() {
                        //     warn!("Failed to broadcast message: No active receivers?");
                        // }
                    }
                    Message::Binary(_) => {
                        warn!("Received unexpected binary message from client");
                    }
                    Message::Ping(_) => {
                        // Axum handles Pongs automatically
                        info!("Received Ping from client");
                    }
                    Message::Pong(_) => {
                         info!("Received Pong from client");
                    }
                    Message::Close(_) => {
                        info!("Client requested WebSocket close");
                        break; // Exit loop to close connection
                    }
                }
            }

            // Client disconnected without sending a Close message
            else => {
                info!("WebSocket client disconnected");
                break;
            }
        }
    }
    info!("WebSocket connection closed");
}

// Modify function signature to accept AgentManager
pub async fn start_web_server(port: u16, agent_manager: Arc<AgentManager>) -> Result<()> {
    let templates = create_minijinja_env().context("Failed to initialize template engine")?;
    // Create a broadcast channel for WebSocket messages
    let (broadcast_tx, _) = broadcast::channel::<BroadcastMessage>(100); // Capacity of 100 messages

    let state = AppState {
        templates: Arc::new(templates),
        broadcast_tx, // Add sender to state
        agent_manager, // Add agent manager to state
    };

    // Serve static files from the `static` directory
    let static_files_service = ServeDir::new("static")
        .not_found_service(tower::service_fn(|_| async {
            Ok::<_, std::convert::Infallible>(
                // Use axum's body type which is compatible with hyper v1
                axum::response::Response::builder()
                    .status(axum::http::StatusCode::NOT_FOUND)
                    .body(axum::body::Body::from("Not Found"))
                    .unwrap(),
            )
        }));

    // Build our application router
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/ws", get(ws_handler)) // WebSocket route
        .route("/api/agents/status", get(api_agent_status_handler)) // API route for status
        .route("/api/synthesize-goal", axum::routing::post(synthesize_goal_handler)) // Add synthesis route
        // Route for static files must be nested under a path like /static
        // or it will conflict with other routes.
        .nest_service("/static", static_files_service)
        .with_state(state)
        .layer(TraceLayer::new_for_http()); // Add request logging

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Web server listening on http://{}", addr);

    // Bind using tokio::net::TcpListener
    let listener = tokio::net::TcpListener::bind(addr).await
        .context(format!("Failed to bind to address {}", addr))?;

    // Use axum::serve to run the application
    serve(listener, app.into_make_service())
        .await
        .context("Web server failed")?;

    Ok(())
}
