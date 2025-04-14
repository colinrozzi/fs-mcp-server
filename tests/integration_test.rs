use anyhow::Result;
use mcp_client::{ClientBuilder, transport::StdioTransport};
use serde_json::json;
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
use tempfile::TempDir;
use tokio;

/// Setup a temporary directory with test files
fn setup_test_directory() -> Result<TempDir> {
    let temp_dir = tempfile::tempdir()?;
    
    // Create some test files and directories
    std::fs::write(
        temp_dir.path().join("test1.txt"),
        "This is a test file with some content.\nMultiple lines of text.\nSearchable pattern here."
    )?;
    
    std::fs::create_dir(temp_dir.path().join("subdir"))?;
    
    std::fs::write(
        temp_dir.path().join("subdir").join("test2.txt"),
        "Another test file in a subdirectory.\nWith more searchable content."
    )?;
    
    Ok(temp_dir)
}

/// Run integration tests for the filesystem MCP server
#[tokio::test]
async fn test_filesystem_server() -> Result<()> {
    // Get the path to the compiled server binary
    let server_executable = env::current_dir()?
        .join("target/debug/fs-mcp-server");
    
    // Setup test directory
    let test_dir = setup_test_directory()?;
    let test_dir_path = test_dir.path().to_str().unwrap().to_string();
    
    // Connect to the server
    let (transport, mut receiver) = StdioTransport::new(
        server_executable.to_str().unwrap(),
        vec![
            "--root-dir".to_string(),
            test_dir_path.clone(),
            "--log-level".to_string(),
            "info".to_string(),
        ],
    );
    
    let client = Arc::new(ClientBuilder::new("test-client", "0.1.0")
        .with_transport(transport)
        .build()?);
    
    // Start message handling
    let client_for_handler = client.clone();
    tokio::spawn(async move {
        while let Some(message) = receiver.recv().await {
            if let Err(e) = client_for_handler.handle_message(message).await {
                eprintln!("Error handling message: {}", e);
            }
        }
    });
    
    // Initialize the client
    let init_result = client.initialize().await?;
    println!("Connected to: {} v{}", init_result.server_info.name, init_result.server_info.version);
    
    // List available tools
    let tools = client.list_tools().await?;
    println!("Available tools: {}", tools.tools.len());
    
    for tool in &tools.tools {
        println!("Tool: {} - {}", tool.name, tool.description.as_deref().unwrap_or(""));
    }
    
    // Test 1: List files in root directory
    println!("Testing fs.list...");
    let list_result = client.call_tool("fs.list", &json!({
        "path": ".",
        "recursive": false
    })).await?;
    
    // Extract text content from the result
    let list_content = match &list_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => text,
        _ => panic!("Expected text content"),
    };
    
    // Verify the list contains our test files
    assert!(list_content.contains("test1.txt"));
    assert!(list_content.contains("subdir"));
    
    // Test 2: Read file content
    println!("Testing fs.read...");
    let read_result = client.call_tool("fs.read", &json!({
        "path": "test1.txt"
    })).await?;
    
    // Extract text content from the result
    let read_content = match &read_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => text,
        _ => panic!("Expected text content"),
    };
    
    // Verify the file content
    assert!(read_content.contains("This is a test file"));
    assert!(read_content.contains("Searchable pattern"));
    
    // Test 3: Search for content
    println!("Testing fs.search...");
    let search_result = client.call_tool("fs.search", &json!({
        "root_path": ".",
        "pattern": "searchable",
        "recursive": true
    })).await?;
    
    // Extract text content from the result
    let search_content = match &search_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => text,
        _ => panic!("Expected text content"),
    };
    
    // Verify search found matches in both files
    assert!(search_content.contains("test1.txt"));
    assert!(search_content.contains("test2.txt"));
    assert!(search_content.contains("Searchable pattern here"));
    
    // Shutdown client
    println!("Tests successful, shutting down...");
    client.shutdown().await?;
    
    Ok(())
}
