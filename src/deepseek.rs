use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use reqwest;
use tokio::sync::mpsc;
use futures_util::StreamExt;

#[derive(Debug, Clone, Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    model: String,
    created_at: String,
    response: String,
    done: bool,
}

#[derive(Debug, Deserialize)]
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

/// Analyze Claude's message to determine if it's asking a question or needs documentation
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
    
    if is_question {
        let doc_hint = if needs_docs {
            Some("Consider using the deepwiki MCP tool to look up relevant documentation.".to_string())
        } else {
            None
        };
        (true, doc_hint)
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
4. Always be helpful and constructive
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
        "model": "deepseek-r1:8b",
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
                                
                                // Send text update
                                let _ = tx.send(DeepSeekMessage::Text {
                                    text: resp.response,
                                    is_thinking: in_thinking,
                                }).await;
                                
                                // If we transitioned thinking states, notify
                                if was_thinking != in_thinking {
                                    let _ = tx.send(DeepSeekMessage::Start { 
                                        is_thinking: in_thinking 
                                    }).await;
                                }
                                
                                if resp.done {
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
4. Always be helpful and constructive
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
        "model": "deepseek-r1:8b",
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