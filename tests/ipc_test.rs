use tokio::net::{UnixListener, UnixStream};
use tokio::io::{AsyncWriteExt, AsyncBufReadExt, BufReader};
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

#[cfg(test)]
mod ipc_tests {
    use super::*;

    #[tokio::test]
    async fn test_unix_socket_creation_and_cleanup() {
        let session_id = "test-session-cleanup";
        let socket_path = format!("/tmp/veda-{}.sock", session_id);
        
        // Remove existing socket if it exists
        let _ = std::fs::remove_file(&socket_path);
        
        // Create listener
        let listener = UnixListener::bind(&socket_path).unwrap();
        
        // Verify socket file exists
        assert!(std::path::Path::new(&socket_path).exists());
        
        // Drop listener
        drop(listener);
        
        // Clean up
        std::fs::remove_file(&socket_path).unwrap();
        
        // Verify socket is removed
        assert!(!std::path::Path::new(&socket_path).exists());
    }

    #[tokio::test]
    async fn test_ipc_message_exchange() {
        let session_id = "test-session-exchange";
        let socket_path = format!("/tmp/veda-{}.sock", session_id);
        
        // Clean up any existing socket
        let _ = std::fs::remove_file(&socket_path);
        
        // Start server
        let listener = UnixListener::bind(&socket_path).unwrap();
        
        // Server task
        let server_handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = socket.split();
            let mut reader = BufReader::new(reader);
            
            // Read message
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            
            let msg: serde_json::Value = serde_json::from_str(&line).unwrap();
            
            // Verify message content
            assert_eq!(msg["type"], "test_message");
            assert_eq!(msg["session_id"], session_id);
            
            // Send response
            let response = "✅ Test response\n";
            writer.write_all(response.as_bytes()).await.unwrap();
        });
        
        // Client connection
        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        
        // Send message
        let test_msg = json!({
            "type": "test_message",
            "session_id": session_id,
            "data": "test data"
        });
        
        let msg_str = format!("{}\n", serde_json::to_string(&test_msg).unwrap());
        client.write_all(msg_str.as_bytes()).await.unwrap();
        
        // Read response
        let (reader, _) = client.split();
        let mut reader = BufReader::new(reader);
        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        
        assert_eq!(response.trim(), "✅ Test response");
        
        // Wait for server to complete
        server_handle.await.unwrap();
        
        // Clean up
        let _ = std::fs::remove_file(&socket_path);
    }

    #[tokio::test]
    async fn test_multiple_sessions_isolation() {
        let sessions = vec!["session-iso-1", "session-iso-2", "session-iso-3"];
        let mut handles = vec![];
        
        // Create listeners for each session
        for session_id in &sessions {
            let socket_path = format!("/tmp/veda-{}.sock", session_id);
            let _ = std::fs::remove_file(&socket_path);
            
            let listener = UnixListener::bind(&socket_path).unwrap();
            let session_id = session_id.to_string();
            
            let handle = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (reader, mut writer) = socket.split();
                let mut reader = BufReader::new(reader);
                
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                
                let msg: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(msg["session_id"], session_id);
                
                // Echo back the session ID
                writer.write_all(format!("{}\n", session_id).as_bytes()).await.unwrap();
            });
            
            handles.push(handle);
        }
        
        // Connect to each session and verify isolation
        for session_id in &sessions {
            let socket_path = format!("/tmp/veda-{}.sock", session_id);
            let mut client = UnixStream::connect(&socket_path).await.unwrap();
            
            // Send message with session ID
            let msg = json!({
                "type": "test",
                "session_id": session_id
            });
            
            client.write_all(format!("{}\n", serde_json::to_string(&msg).unwrap()).as_bytes()).await.unwrap();
            
            // Read response and verify it matches
            let (reader, _) = client.split();
            let mut reader = BufReader::new(reader);
            let mut response = String::new();
            reader.read_line(&mut response).await.unwrap();
            
            assert_eq!(response.trim(), *session_id);
        }
        
        // Wait for all servers
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Clean up
        for session_id in &sessions {
            let socket_path = format!("/tmp/veda-{}.sock", session_id);
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[tokio::test]
    async fn test_ipc_timeout_handling() {
        let session_id = "test-timeout";
        let socket_path = format!("/tmp/veda-{}.sock", session_id);
        let _ = std::fs::remove_file(&socket_path);
        
        // Try to connect without server running
        let connect_result = timeout(
            Duration::from_millis(100),
            UnixStream::connect(&socket_path)
        ).await;
        
        // Connection should either timeout or fail
        match connect_result {
            Err(_) => {}, // Timeout - expected
            Ok(Err(_)) => {}, // Connection error - expected
            Ok(Ok(_)) => panic!("Should not connect when no server is running"),
        }
        
        // Now start server and verify connection works
        let _listener = UnixListener::bind(&socket_path).unwrap();
        
        let connect_result = timeout(
            Duration::from_millis(100),
            UnixStream::connect(&socket_path)
        ).await;
        
        assert!(connect_result.is_ok(), "Should connect when server is running");
        
        // Clean up
        let _ = std::fs::remove_file(&socket_path);
    }

    #[tokio::test]
    async fn test_spawn_instances_message_format() {
        // Test the exact message format used by MCP server
        let msg = json!({
            "type": "spawn_instances",
            "session_id": "test-session",
            "task_description": "Implement parallel features",
            "num_instances": 3
        });
        
        // Verify all required fields are present
        assert_eq!(msg["type"], "spawn_instances");
        assert_eq!(msg["session_id"], "test-session");
        assert_eq!(msg["task_description"], "Implement parallel features");
        assert_eq!(msg["num_instances"], 3);
    }

    #[tokio::test]
    async fn test_list_instances_message_format() {
        let msg = json!({
            "type": "list_instances",
            "session_id": "test-session"
        });
        
        assert_eq!(msg["type"], "list_instances");
        assert_eq!(msg["session_id"], "test-session");
    }

    #[tokio::test]
    async fn test_close_instance_message_format() {
        let msg = json!({
            "type": "close_instance",
            "session_id": "test-session",
            "instance_name": "Claude 2-A"
        });
        
        assert_eq!(msg["type"], "close_instance");
        assert_eq!(msg["session_id"], "test-session");
        assert_eq!(msg["instance_name"], "Claude 2-A");
    }
}