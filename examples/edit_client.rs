use anyhow::Result;
use mcp_client::{ClientBuilder, transport::StdioTransport};
use serde_json::json;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Get the path to the compiled server binary
    let server_executable = env::current_dir()?
        .join("target/debug/fs-mcp-server");
    
    println!("Starting server: {:?}", server_executable);
    
    // Get current directory as the allowed directory
    let current_dir = env::current_dir()?;
    
    println!("Using allowed directory: {}", current_dir.display());
    
    // Create a temporary test file
    let test_file = current_dir.join("edit_test.txt");
    let initial_content = "Line 1: This is a test file.\nLine 2: It has multiple lines.\nLine 3: We will edit this file.\nLine 4: Without rewriting the whole thing.\nLine 5: Using the new edit tool.";
    
    println!("Creating test file: {}", test_file.display());
    fs::write(&test_file, initial_content)?;
    
    // Start the server with the current directory as allowed
    let mut server_args = vec![
        "--allowed-dirs".to_string(),
        current_dir.to_str().unwrap().to_string(),
        "--log-level".to_string(),
        "debug".to_string(),
    ];
    
    // Connect to the server
    let (transport, mut receiver) = StdioTransport::new(
        server_executable.to_str().unwrap(),
        server_args,
    );
    
    let client = Arc::new(ClientBuilder::new("edit-client", "0.1.0")
        .with_transport(transport)
        .build()?);
    
    // Start message handling
    let client_for_handler = client.clone();
    let handler = tokio::spawn(async move {
        while let Some(message) = receiver.recv().await {
            if let Err(e) = client_for_handler.handle_message(message).await {
                eprintln!("Error handling message: {}", e);
            }
        }
    });
    
    // Initialize the client
    let init_result = client.initialize().await?;
    println!("Connected to: {} v{}", init_result.server_info.name, init_result.server_info.version);
    
    // First, read the file to confirm its initial content
    println!("\nReading initial file content...");
    let read_result = client.call_tool("read", &json!({
        "path": test_file.to_str().unwrap()
    })).await?;
    
    match &read_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Initial content:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Example 1: Replace text
    println!("\nPerforming replace operation...");
    let edit_result = client.call_tool("edit", &json!({
        "path": test_file.to_str().unwrap(),
        "operations": [
            {
                "type": "replace",
                "find": "test file",
                "replace": "sample document",
                "occurrence": 0,
                "case_sensitive": true
            }
        ],
        "backup": true
    })).await?;
    
    match &edit_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Replace operation result:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Read file after first edit
    println!("\nReading file after replace operation...");
    let read_result = client.call_tool("read", &json!({
        "path": test_file.to_str().unwrap()
    })).await?;
    
    match &read_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Content after replace:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Example 2: Replace all occurrences
    println!("\nReplacing all occurrences of 'Line'...");
    let edit_result = client.call_tool("edit", &json!({
        "path": test_file.to_str().unwrap(),
        "operations": [
            {
                "type": "replace",
                "find": "Line",
                "replace": "Item",
                "occurrence": -1
            }
        ]
    })).await?;
    
    match &edit_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Replace all operation result:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Example 3: Insert text at position
    println!("\nInserting text at specific position...");
    let edit_result = client.call_tool("edit", &json!({
        "path": test_file.to_str().unwrap(),
        "operations": [
            {
                "type": "insert",
                "position": 0,
                "content": "--- DOCUMENT START ---\n"
            }
        ]
    })).await?;
    
    match &edit_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Insert operation result:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Example 4: Replace lines
    println!("\nReplacing specific lines...");
    let edit_result = client.call_tool("edit", &json!({
        "path": test_file.to_str().unwrap(),
        "operations": [
            {
                "type": "replace_lines",
                "start_line": 2,
                "end_line": 3,
                "content": "Item 3: This is a completely replaced line.\nItem 4: This is also new content."
            }
        ]
    })).await?;
    
    match &edit_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Replace lines operation result:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Example 5: Delete text
    println!("\nDeleting text range...");
    let edit_result = client.call_tool("edit", &json!({
        "path": test_file.to_str().unwrap(),
        "operations": [
            {
                "type": "delete",
                "start": 0,
                "end": 23
            }
        ]
    })).await?;
    
    match &edit_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Delete operation result:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Example 6: Multiple operations in a single call
    println!("\nPerforming multiple operations in a single call...");
    let edit_result = client.call_tool("edit", &json!({
        "path": test_file.to_str().unwrap(),
        "operations": [
            {
                "type": "replace",
                "find": "Item",
                "replace": "Section",
                "occurrence": -1
            },
            {
                "type": "insert",
                "position": 0,
                "content": "--- REVISED DOCUMENT ---\n"
            },
            {
                "type": "insert",
                "position": 9999, // End of file (will clamp to file length)
                "content": "\n--- END OF DOCUMENT ---"
            }
        ]
    })).await?;
    
    match &edit_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Multiple operations result:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Read final file content
    println!("\nReading final file content after all edits...");
    let read_result = client.call_tool("read", &json!({
        "path": test_file.to_str().unwrap()
    })).await?;
    
    match &read_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Final content:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Cleanup: Delete the test file
    println!("\nCleaning up test file...");
    let delete_result = client.call_tool("delete", &json!({
        "path": test_file.to_str().unwrap()
    })).await?;
    
    match &delete_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Delete file result:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Delete the backup file if it exists
    let backup_file = PathBuf::from(format!("{}.bak", test_file.to_str().unwrap()));
    if backup_file.exists() {
        let delete_result = client.call_tool("delete", &json!({
            "path": backup_file.to_str().unwrap()
        })).await?;
        
        match &delete_result.content[0] {
            mcp_protocol::types::tool::ToolContent::Text { text } => {
                println!("Delete backup file result:\n{}", text);
            },
            _ => println!("Unexpected content type"),
        }
    }
    
    // Shutdown client
    println!("\nDemo complete, shutting down...");
    client.shutdown().await?;
    
    // Ensure the handler completes
    handler.await?;
    
    Ok(())
}