use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use reqwest;
use tokio::sync::mpsc;
use futures_util::StreamExt;

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OllamaResponse {
    model: String,
    created_at: String,
    response: String,
    done: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OllamaStreamResponse {
    model: String,
    created_at: String,
    response: String,
    done: bool,
}

#[derive(Debug, Clone)]
pub enum DeepSeekMessage {
    Start { is_thinking: bool },
    Text { text: String, is_thinking: bool },
    End,
    Error { error: String },
}

/// Analyze Claude's message to determine if it's asking a question or needs documentation/project management
pub fn analyze_claude_message(message: &str) -> (bool, Option<String>) {
    let lowercase = message.to_lowercase();
    
    // Check if Claude is asking about something
    let is_question = lowercase.contains("?") ||
        lowercase.contains("what ") ||
        lowercase.contains("how ") ||
        lowercase.contains("where ") ||
        lowercase.contains("when ") ||
        lowercase.contains("why ") ||
        lowercase.contains("which ") ||
        lowercase.contains("could you") ||
        lowercase.contains("can you") ||
        lowercase.contains("please") ||
        lowercase.contains("i need") ||
        lowercase.contains("i'm looking") ||
        lowercase.contains("help me");
    
    // Check if it might need documentation
    let needs_docs = lowercase.contains("documentation") ||
        lowercase.contains("docs") ||
        lowercase.contains("api") ||
        lowercase.contains("library") ||
        lowercase.contains("framework") ||
        lowercase.contains("how to use") ||
        lowercase.contains("example") ||
        lowercase.contains("tutorial");
        
    // Check if it might need project management
    let needs_taskmaster = lowercase.contains("task") ||
        lowercase.contains("todo") ||
        lowercase.contains("next") ||
        lowercase.contains("progress") ||
        lowercase.contains("prd") ||
        lowercase.contains("requirements") ||
        lowercase.contains("project") ||
        lowercase.contains("what should i") ||
        lowercase.contains("what's next") ||
        lowercase.contains("status");
    
    if is_question {
        let hint = if needs_taskmaster {
            Some("Consider using TaskMaster AI tools (mcp__taskmaster-ai__get_tasks, mcp__taskmaster-ai__next_task) to check project progress.".to_string())
        } else if needs_docs {
            Some("Consider using the deepwiki MCP tool to look up relevant documentation.".to_string())
        } else {
            None
        };
        (true, hint)
    } else {
        (false, None)
    }
}

/// Check if Claude is mentioning tool permission issues and get DeepSeek's judgment
pub async fn check_tool_permission_issue(message: &str, attempted_tools: &[String]) -> Result<Option<Vec<String>>> {
    tracing::info!("Checking for tool permission issues in Claude's message with attempted tools: {:?}", attempted_tools);
    
    // Ask DeepSeek to analyze if Claude is having permission issues
    let prompt = format!(
        r#"Claude just attempted to use these tools: {:?}
Then Claude said: "{}"

Analyze if Claude is indicating it needs permission to use the tools it just attempted.

Instructions:
1. If Claude mentions needing permission, not being allowed, or the file/tool being restricted, respond with: TOOLS_NEEDED: followed by the tools from the attempted list
2. If Claude is NOT mentioning permission issues, respond with: NO_PERMISSION_ISSUE
3. Common permission phrases include: "need permission", "not allowed", "cannot create", "restricted", "access denied"
4. If Claude tried to use "Write" and mentions needing permission to create files, that counts as needing the Write tool

Your response (one line only):"#,
        attempted_tools,
        message
    );
    
    let request_body = json!({
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
        return Ok(None);
    }
    
    let ollama_response: OllamaResponse = response.json().await?;
    let response_text = ollama_response.response.trim();
    
    tracing::debug!("DeepSeek permission check response: {}", response_text);
    
    // Extract the final verdict, ignoring chain of thought
    let verdict = if let Some(idx) = response_text.rfind("TOOLS_NEEDED:") {
        response_text[idx..].trim()
    } else if response_text.contains("NO_PERMISSION_ISSUE") {
        "NO_PERMISSION_ISSUE"
    } else {
        response_text
    };
    
    if verdict.starts_with("TOOLS_NEEDED:") {
        let tools_part = verdict.trim_start_matches("TOOLS_NEEDED:").trim();
        let tools: Vec<String> = tools_part
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        
        if tools.is_empty() {
            Ok(None)
        } else {
            tracing::info!("DeepSeek identified tools needing permission: {:?}", tools);
            Ok(Some(tools))
        }
    } else {
        Ok(None)
    }
}

/// Detect if response contains chain-of-thought markers
fn is_chain_of_thought(text: &str) -> bool {
    let lowercase = text.to_lowercase();
    lowercase.contains("<think>") || 
    lowercase.contains("</think>") ||
    lowercase.contains("let me think") ||
    lowercase.contains("let's think") ||
    lowercase.contains("first, i") ||
    lowercase.contains("step 1:") ||
    lowercase.contains("step 2:")
}

/// Generate a stall intervention response using DeepSeek-R1:8b through Ollama API
pub async fn generate_deepseek_stall_response(
    claude_message: &str,
    user_context: &str,
    tx: mpsc::Sender<DeepSeekMessage>,
) -> Result<()> {
    tracing::info!("Generating stall intervention response for Claude's last message: {}", claude_message);
    
    // First, try to get current project status via TaskMaster AI
    let project_status = get_taskmaster_status().await.unwrap_or_else(|e| {
        tracing::warn!("Failed to get TaskMaster status: {}", e);
        "No task information available".to_string()
    });
    
    // Construct a prompt focused on progress and goals
    let prompt = format!(
        r#"You are assisting in a conversation between a user and Claude (an AI assistant). The conversation has stalled.

Claude's last message was: "{}"

The user's recent requests were: "{}"

Current project status from TaskMaster AI:
{}

The conversation has been idle, suggesting Claude may be waiting for input or may have completed a task without confirming success.

Your goal is to help move the conversation forward toward completing the PRD (Product Requirements Document), testing the code, and if there's a web UI involved, doing thorough testing with Playwright.

You have access to TaskMaster AI tools for project management. Consider suggesting Claude use these tools to check progress:

- mcp__taskmaster-ai__get_tasks: View current task list and status
- mcp__taskmaster-ai__next_task: Find the next task to work on
- mcp__taskmaster-ai__parse_prd: Parse PRD document to generate tasks
- mcp__taskmaster-ai__set_task_status: Update task completion status

Please analyze the situation and provide ONE of these responses:

1. If Claude used tools and may have completed a task, ask Claude to confirm success and update task status
2. If Claude seems to be waiting, suggest checking TaskMaster for next tasks or parsing the PRD
3. If testing is needed, remind Claude to run tests and verify functionality  
4. If there's a web UI, suggest using Playwright for comprehensive testing
5. If TaskMaster needs initialization, tell Claude to use mcp__taskmaster-ai__initialize_project
6. If no PRD exists, suggest Claude create a Product Requirements Document at scripts/prd.txt
7. If PRD exists but no tasks, suggest using mcp__taskmaster-ai__parse_prd to generate tasks
8. If unclear about project status, suggest using TaskMaster tools to check current tasks and progress

Keep your response concise and action-oriented. Focus on making progress toward completing the project using available tools.

MESSAGE_TO_CLAUDE_WITH_VERDICT:"#,
        claude_message,
        user_context,
        project_status
    );
    
    // Create the request body for Ollama API with streaming
    let request_body = json!({
        "model": "gemma3:12b",
        "prompt": prompt,
        "stream": true
    });
    
    // Make HTTP request to Ollama API
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:11434/api/generate")
        .json(&request_body)
        .send()
        .await?;
    
    if !response.status().is_success() {
        let error_text = response.text().await?;
        tracing::error!("Ollama API error: {}", error_text);
        let _ = tx.send(DeepSeekMessage::Error { error: error_text.clone() }).await;
        return Err(anyhow::anyhow!("Ollama API error: {}", error_text));
    }
    
    // Start streaming
    let _ = tx.send(DeepSeekMessage::Start { is_thinking: false }).await;
    
    let mut stream = response.bytes_stream();
    let mut accumulated_text = String::new();
    let mut in_thinking = false;
    let mut text_buffer = String::new();
    let mut last_send = std::time::Instant::now();
    
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    // Parse each line as JSON
                    for line in text.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        
                        match serde_json::from_str::<OllamaStreamResponse>(line) {
                            Ok(resp) => {
                                accumulated_text.push_str(&resp.response);
                                
                                // Check if we're in chain-of-thought
                                let was_thinking = in_thinking;
                                in_thinking = is_chain_of_thought(&accumulated_text);
                                
                                // Buffer text and send in chunks to reduce UI spam
                                text_buffer.push_str(&resp.response);
                                
                                // Send buffered text every 100ms or when we have substantial content
                                let should_send = text_buffer.len() >= 50 || 
                                                last_send.elapsed() >= std::time::Duration::from_millis(100) ||
                                                resp.done ||
                                                was_thinking != in_thinking;
                                
                                if should_send && !text_buffer.is_empty() {
                                    let _ = tx.send(DeepSeekMessage::Text {
                                        text: text_buffer.clone(),
                                        is_thinking: in_thinking,
                                    }).await;
                                    text_buffer.clear();
                                    last_send = std::time::Instant::now();
                                }
                                
                                // If we transitioned thinking states, notify
                                if was_thinking != in_thinking {
                                    let _ = tx.send(DeepSeekMessage::Start { 
                                        is_thinking: in_thinking 
                                    }).await;
                                }
                                
                                if resp.done {
                                    // Send any remaining buffered text
                                    if !text_buffer.is_empty() {
                                        let _ = tx.send(DeepSeekMessage::Text {
                                            text: text_buffer,
                                            is_thinking: in_thinking,
                                        }).await;
                                    }
                                    let _ = tx.send(DeepSeekMessage::End).await;
                                    return Ok(());
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse JSON line: {} - Error: {}", line, e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Stream error: {}", e);
                let _ = tx.send(DeepSeekMessage::Error { 
                    error: e.to_string() 
                }).await;
                return Err(anyhow::anyhow!("Stream error: {}", e));
            }
        }
    }
    
    Ok(())
}

/// Get current project status from TaskMaster AI via direct MCP calls
async fn get_taskmaster_status() -> Result<String> {
    // Get current working directory for project root
    let current_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    
    tracing::info!("Getting TaskMaster status via MCP for project: {}", current_dir);
    
    // First check if TaskMaster is initialized
    match ensure_taskmaster_initialized(&current_dir).await {
        Ok(init_message) => {
            if !init_message.is_empty() {
                return Ok(format!("TaskMaster Initialization: {}", init_message));
            }
        }
        Err(e) => {
            tracing::error!("Failed to initialize TaskMaster: {}", e);
            return Ok(format!("TaskMaster Error: Failed to initialize - {}", e));
        }
    }
    
    let mut status_info = Vec::new();
    
    // Call TaskMaster AI get_tasks
    match call_taskmaster_mcp("get_tasks", &json!({
        "projectRoot": current_dir
    })).await {
        Ok(tasks_response) => {
            status_info.push(format!("Current Tasks: {}", tasks_response));
        }
        Err(e) => {
            tracing::warn!("Failed to get tasks: {}", e);
            status_info.push("Tasks: Unable to retrieve current tasks".to_string());
        }
    }
    
    // Call TaskMaster AI next_task
    match call_taskmaster_mcp("next_task", &json!({
        "projectRoot": current_dir
    })).await {
        Ok(next_response) => {
            status_info.push(format!("Next Task: {}", next_response));
        }
        Err(e) => {
            tracing::warn!("Failed to get next task: {}", e);
            status_info.push("Next Task: Unable to determine next task".to_string());
        }
    }
    
    Ok(status_info.join("\n"))
}

/// Ensure TaskMaster AI is initialized, and if not, set it up
async fn ensure_taskmaster_initialized(project_root: &str) -> Result<String> {
    use std::path::Path;
    
    // Check if .taskmasterconfig exists
    let config_path = Path::new(project_root).join(".taskmasterconfig");
    if config_path.exists() {
        tracing::debug!("TaskMaster already initialized at {}", project_root);
        return Ok(String::new()); // Already initialized
    }
    
    tracing::info!("TaskMaster not initialized, setting up project at {}", project_root);
    
    // Initialize TaskMaster
    match call_taskmaster_mcp("initialize_project", &json!({
        "projectRoot": project_root,
        "yes": true,
        "skipInstall": false
    })).await {
        Ok(init_response) => {
            tracing::info!("TaskMaster initialized successfully: {}", init_response);
            
            // Check if PRD exists
            let prd_path = Path::new(project_root).join("scripts").join("prd.txt");
            if !prd_path.exists() {
                tracing::info!("No PRD found, suggesting Claude create one");
                return Ok(format!(
                    "TaskMaster initialized. No PRD found at scripts/prd.txt. Claude should create a Product Requirements Document to define project goals and features."
                ));
            } else {
                // PRD exists, suggest parsing it
                return Ok(format!(
                    "TaskMaster initialized. PRD found at scripts/prd.txt. Claude should parse it using mcp__taskmaster-ai__parse_prd to generate tasks."
                ));
            }
        }
        Err(e) => {
            tracing::error!("Failed to initialize TaskMaster: {}", e);
            return Err(anyhow::anyhow!("TaskMaster initialization failed: {}", e));
        }
    }
}

/// Get TaskMaster AI documentation via DeepWiki MCP
#[allow(dead_code)]
async fn get_taskmaster_docs() -> Result<String> {
    tracing::info!("Getting TaskMaster AI documentation via DeepWiki MCP");
    
    match call_deepwiki_mcp("read_wiki_contents", &json!({
        "repoName": "eyaltoledano/claude-task-master"
    })).await {
        Ok(docs) => Ok(docs),
        Err(e) => {
            tracing::warn!("Failed to get TaskMaster docs: {}", e);
            Ok("TaskMaster AI documentation not available".to_string())
        }
    }
}

/// Call DeepWiki via SSE endpoint (as per MCP config)
#[allow(dead_code)]
async fn call_deepwiki_mcp(method: &str, params: &serde_json::Value) -> Result<String> {
    let client = reqwest::Client::new();
    
    // Construct MCP JSON-RPC request for SSE
    let request_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": format!("tools/call"),
        "params": {
            "name": format!("mcp__deepwiki__{}", method),
            "arguments": params
        }
    });
    
    // DeepWiki SSE endpoint
    let sse_url = "https://mcp.deepwiki.com/sse";
    
    tracing::debug!("Calling DeepWiki SSE: {} with params: {}", method, params);
    
    let response = client
        .post(sse_url)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&request_body)
        .send()
        .await?;
    
    if response.status().is_success() {
        let response_text = response.text().await?;
        tracing::debug!("DeepWiki SSE response: {}", response_text);
        
        // Parse SSE response - look for data events
        let mut result_content = String::new();
        for line in response_text.lines() {
            if line.starts_with("data: ") {
                let data = &line[6..]; // Remove "data: " prefix
                if let Ok(json_data) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(content) = json_data.get("content") {
                        if let Some(text) = content.get("text") {
                            if let Some(text_str) = text.as_str() {
                                result_content.push_str(text_str);
                            }
                        }
                    }
                }
            }
        }
        
        if !result_content.is_empty() {
            Ok(result_content)
        } else {
            // Fallback: return raw response if parsing fails
            Ok(response_text)
        }
    } else {
        let error_text = response.text().await?;
        tracing::error!("DeepWiki SSE error: {}", error_text);
        Err(anyhow::anyhow!("DeepWiki SSE error: {}", error_text))
    }
}

/// Call TaskMaster AI via npx command (as per MCP config)
async fn call_taskmaster_mcp(method: &str, params: &serde_json::Value) -> Result<String> {
    use tokio::process::Command;
    use std::process::Stdio;
    
    // Get current working directory for project root
    let current_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    
    tracing::debug!("Calling TaskMaster AI via npx: {} with params: {}", method, params);
    
    // Construct arguments for task-master-ai command
    let mut args = vec![
        "-y".to_string(),
        "--package=task-master-ai".to_string(), 
        "task-master-ai".to_string(),
        method.to_string(),
    ];
    
    // Add parameters as command line arguments
    if let Some(project_root) = params.get("projectRoot") {
        if let Some(root_str) = project_root.as_str() {
            args.extend(vec!["--projectRoot".to_string(), root_str.to_string()]);
        }
    }
    
    // Add other common parameters
    for (key, value) in params.as_object().unwrap_or(&serde_json::Map::new()) {
        if key != "projectRoot" {
            match value {
                serde_json::Value::String(val_str) => {
                    args.extend(vec![format!("--{}", key), val_str.to_string()]);
                }
                serde_json::Value::Bool(true) => {
                    args.push(format!("--{}", key));
                }
                serde_json::Value::Bool(false) => {
                    // Skip false boolean flags
                }
                serde_json::Value::Number(num) => {
                    args.extend(vec![format!("--{}", key), num.to_string()]);
                }
                _ => {
                    // Skip other types for now
                }
            }
        }
    }
    
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30), // 30 second timeout
        Command::new("npx")
            .args(&args)
            .current_dir(&current_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
    ).await;
    
    let output = match output {
        Ok(result) => result?,
        Err(_) => {
            tracing::error!("TaskMaster AI command timed out after 30 seconds");
            return Err(anyhow::anyhow!("TaskMaster AI command timed out"));
        }
    };
    
    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout);
        tracing::debug!("TaskMaster AI response: {}", result);
        Ok(result.trim().to_string())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        tracing::error!("TaskMaster AI error: {}", error);
        Err(anyhow::anyhow!("TaskMaster AI command failed: {}", error))
    }
}

/// Generate a streaming response using DeepSeek-R1:8b through Ollama API
pub async fn generate_deepseek_response_stream(
    claude_message: &str,
    user_context: &str,
    tx: mpsc::Sender<DeepSeekMessage>,
) -> Result<()> {
    tracing::info!("Generating streaming DeepSeek response for Claude's message: {}", claude_message);
    
    // Analyze if documentation might be needed
    let (_, doc_hint) = analyze_claude_message(claude_message);
    
    // Construct the prompt for DeepSeek
    let mut prompt = format!(
        r#"You are assisting in a conversation between a user and Claude (an AI assistant). 
Claude just said: "{}"

The user's original request was: "{}"

Please provide a helpful response to Claude's question or statement. Be concise and direct.
If Claude seems to need specific technical information, suggest using the deepwiki MCP tool.

Important instructions:
1. Keep your response brief and to the point
2. If Claude needs documentation about a library/framework, tell it to use: mcp__deepwiki__read_wiki_contents or mcp__deepwiki__ask_question
3. If Claude is asking about code structure or implementation details, suggest specific files or approaches
4. For project management, suggest TaskMaster AI tools: mcp__taskmaster-ai__get_tasks, mcp__taskmaster-ai__next_task, mcp__taskmaster-ai__parse_prd
5. Always be helpful and constructive
"#,
        claude_message,
        user_context
    );
    
    if let Some(hint) = doc_hint {
        prompt.push_str(&format!("\nNote: {}\n", hint));
    }
    
    prompt.push_str("\nYour response to Claude:");
    
    // Create the request body for Ollama API with streaming
    let request_body = json!({
        "model": "gemma3:12b",
        "prompt": prompt,
        "stream": true
    });
    
    // Make HTTP request to Ollama API
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:11434/api/generate")
        .json(&request_body)
        .send()
        .await?;
    
    if !response.status().is_success() {
        let error_text = response.text().await?;
        tracing::error!("Ollama API error: {}", error_text);
        let _ = tx.send(DeepSeekMessage::Error { error: error_text.clone() }).await;
        return Err(anyhow::anyhow!("Ollama API error: {}", error_text));
    }
    
    // Start streaming
    let _ = tx.send(DeepSeekMessage::Start { is_thinking: false }).await;
    
    let mut stream = response.bytes_stream();
    let mut accumulated_text = String::new();
    let mut in_thinking = false;
    let mut text_buffer = String::new();
    let mut last_send = std::time::Instant::now();
    
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    // Parse each line as JSON
                    for line in text.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        
                        match serde_json::from_str::<OllamaStreamResponse>(line) {
                            Ok(resp) => {
                                accumulated_text.push_str(&resp.response);
                                
                                // Check if we're in chain-of-thought
                                let was_thinking = in_thinking;
                                in_thinking = is_chain_of_thought(&accumulated_text);
                                
                                // Buffer text and send in chunks to reduce UI spam
                                text_buffer.push_str(&resp.response);
                                
                                // Send buffered text every 100ms or when we have substantial content
                                let should_send = text_buffer.len() >= 50 || 
                                                last_send.elapsed() >= std::time::Duration::from_millis(100) ||
                                                resp.done ||
                                                was_thinking != in_thinking;
                                
                                if should_send && !text_buffer.is_empty() {
                                    let _ = tx.send(DeepSeekMessage::Text {
                                        text: text_buffer.clone(),
                                        is_thinking: in_thinking,
                                    }).await;
                                    text_buffer.clear();
                                    last_send = std::time::Instant::now();
                                }
                                
                                // If we transitioned thinking states, notify
                                if was_thinking != in_thinking {
                                    let _ = tx.send(DeepSeekMessage::Start { 
                                        is_thinking: in_thinking 
                                    }).await;
                                }
                                
                                if resp.done {
                                    // Send any remaining buffered text
                                    if !text_buffer.is_empty() {
                                        let _ = tx.send(DeepSeekMessage::Text {
                                            text: text_buffer,
                                            is_thinking: in_thinking,
                                        }).await;
                                    }
                                    let _ = tx.send(DeepSeekMessage::End).await;
                                    return Ok(());
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse JSON line: {} - Error: {}", line, e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Stream error: {}", e);
                let _ = tx.send(DeepSeekMessage::Error { 
                    error: e.to_string() 
                }).await;
                return Err(anyhow::anyhow!("Stream error: {}", e));
            }
        }
    }
    
    Ok(())
}

/// Generate a response using DeepSeek-R1:8b through Ollama API (non-streaming)
pub async fn generate_deepseek_response(
    claude_message: &str,
    user_context: &str,
) -> Result<String> {
    tracing::info!("Generating DeepSeek response for Claude's message: {}", claude_message);
    
    // Analyze if documentation might be needed
    let (_, doc_hint) = analyze_claude_message(claude_message);
    
    // Construct the prompt for DeepSeek
    let mut prompt = format!(
        r#"You are assisting in a conversation between a user and Claude (an AI assistant). 
Claude just said: "{}"

The user's original request was: "{}"

Please provide a helpful response to Claude's question or statement. Be concise and direct.
If Claude seems to need specific technical information, suggest using the deepwiki MCP tool.

Important instructions:
1. Keep your response brief and to the point
2. If Claude needs documentation about a library/framework, tell it to use: mcp__deepwiki__read_wiki_contents or mcp__deepwiki__ask_question
3. If Claude is asking about code structure or implementation details, suggest specific files or approaches
4. For project management, suggest TaskMaster AI tools: mcp__taskmaster-ai__get_tasks, mcp__taskmaster-ai__next_task, mcp__taskmaster-ai__parse_prd
5. Always be helpful and constructive
"#,
        claude_message,
        user_context
    );
    
    if let Some(hint) = doc_hint {
        prompt.push_str(&format!("\nNote: {}\n", hint));
    }
    
    prompt.push_str("\nYour response to Claude:");
    
    // Create the request body for Ollama API
    let request_body = json!({
        "model": "gemma3:12b",
        "prompt": prompt,
        "stream": false
    });
    
    // Make HTTP request to Ollama API
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
    
    let ollama_response: OllamaResponse = response.json().await?;
    
    Ok(ollama_response.response.trim().to_string())
}

/// Create an instruction prompt for Claude when it needs TaskMaster AI project management
pub fn create_taskmaster_prompt() -> String {
    format!(
        r#"To manage your project tasks and track progress, you should use the TaskMaster AI MCP tools. Here are the key commands:

1. View current tasks and their status:
   Use tool: mcp__taskmaster-ai__get_tasks
   Parameters: {{"projectRoot": "/path/to/project"}}

2. Find the next task to work on:
   Use tool: mcp__taskmaster-ai__next_task  
   Parameters: {{"projectRoot": "/path/to/project"}}

3. Parse a PRD document to generate tasks:
   Use tool: mcp__taskmaster-ai__parse_prd
   Parameters: {{"projectRoot": "/path/to/project", "input": "scripts/prd.txt"}}

4. Update task completion status:
   Use tool: mcp__taskmaster-ai__set_task_status
   Parameters: {{"projectRoot": "/path/to/project", "id": "task_id", "status": "done"}}

5. Get a specific task details:
   Use tool: mcp__taskmaster-ai__get_task
   Parameters: {{"projectRoot": "/path/to/project", "id": "task_id"}}

For more information about TaskMaster AI, you can also check:
   Use tool: mcp__deepwiki__read_wiki_contents
   Parameters: {{"repoName": "eyaltoledano/claude-task-master"}}

Use these tools to stay organized and track your progress toward completing the PRD goals."#
    )
}

/// Create an instruction prompt for Claude when it needs to look up documentation
pub fn create_documentation_prompt(topic: &str) -> String {
    format!(
        r#"To find documentation about {}, you should use the deepwiki MCP tool. Here are the available commands:

1. To see available documentation for a GitHub repo:
   Use tool: mcp__deepwiki__read_wiki_structure
   Parameters: {{"repoName": "owner/repo"}}

2. To read the documentation:
   Use tool: mcp__deepwiki__read_wiki_contents
   Parameters: {{"repoName": "owner/repo"}}

3. To ask a specific question:
   Use tool: mcp__deepwiki__ask_question
   Parameters: {{"repoName": "owner/repo", "question": "your question here"}}

For example, if you need React documentation:
- Use mcp__deepwiki__read_wiki_contents with {{"repoName": "facebook/react"}}

Please use these tools to find the information you need about {}."#,
        topic, topic
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_analyze_claude_message_questions() {
        let (is_question, _) = analyze_claude_message("What is the best way to implement this?");
        assert!(is_question);
        
        let (is_question, _) = analyze_claude_message("How do I use this library?");
        assert!(is_question);
        
        let (is_question, _) = analyze_claude_message("I'll implement that now.");
        assert!(!is_question);
    }
    
    #[test]
    fn test_analyze_documentation_hints() {
        let (_, hint) = analyze_claude_message("How do I use the React documentation?");
        assert!(hint.is_some());
        
        let (_, hint) = analyze_claude_message("What API should I use?");
        assert!(hint.is_some());
        
        let (_, hint) = analyze_claude_message("What time is it?");
        assert!(hint.is_none());
    }
    
    #[test]
    fn test_documentation_prompt() {
        let prompt = create_documentation_prompt("React hooks");
        assert!(prompt.contains("deepwiki"));
        assert!(prompt.contains("React hooks"));
        assert!(prompt.contains("facebook/react"));
    }
}