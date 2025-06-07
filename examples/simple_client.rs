use anyhow::Result;
use modelcontextprotocol_client::{ClientBuilder, transport::StdioTransport};
use serde_json::json;
use std::env;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Get the path to the compiled server binary
    let server_executable = env::current_dir()?
        .join("target/debug/fs-mcp-server");
    
    println!("Starting server: {:?}", server_executable);
    
    // Set up multiple allowed directories for the server
    let current_dir = env::current_dir()?;
    let parent_dir = current_dir.parent().unwrap_or(&current_dir);
    
    // Create a vector of allowed directories
    let allowed_dirs = vec![
        current_dir.to_str().unwrap().to_string(),
        parent_dir.to_str().unwrap().to_string(),
    ];
    
    println!("Allowed directories:");
    for (i, dir) in allowed_dirs.iter().enumerate() {
        println!("  {}: {}", i + 1, dir);
    }
    
    // Start the server with multiple allowed directories
    let server_args = vec![
        "--allowed-dirs".to_string(),
        allowed_dirs.join(","),
        "--log-level".to_string(),
        "debug".to_string(),
    ];
    
    // Connect to the server
    let (transport, mut receiver) = StdioTransport::new(
        server_executable.to_str().unwrap(),
        server_args,
    );
    
    let client = Arc::new(ClientBuilder::new("simple-client", "0.1.0")
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
    
    // List available tools
    let tools = client.list_tools().await?;
    println!("Available tools: {}", tools.tools.len());
    
    for tool in &tools.tools {
        println!("Tool: {} - {}", tool.name, tool.description.as_deref().unwrap_or(""));
    }
    
    // Test list_allowed_dirs
    println!("\nListing all allowed directories...");
    let allowed_dirs_result = client.call_tool("list_allowed_dirs", &json!({})).await?;
    
    match &allowed_dirs_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("Allowed directories:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Test list from current directory
    println!("\nListing files in current directory...");
    let list_result = client.call_tool("list", &json!({
        "path": current_dir.to_str().unwrap(),
        "recursive": false
    })).await?;
    
    match &list_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("List result (current dir):\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Test list from parent directory
    println!("\nListing files in parent directory...");
    let list_result = client.call_tool("list", &json!({
        "path": parent_dir.to_str().unwrap(),
        "recursive": false
    })).await?;
    
    match &list_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("List result (parent dir):\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Test read on Cargo.toml file
    println!("\nReading Cargo.toml file...");
    let read_result = client.call_tool("read", &json!({
        "path": current_dir.join("Cargo.toml").to_str().unwrap()
    })).await?;
    
    match &read_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            // Only show the first 200 characters to avoid flooding the console
            let preview: String = text.chars().take(200).collect();
            println!("Read result (first 200 chars):\n{}", preview);
            println!("... (content truncated)");
        },
        _ => println!("Unexpected content type"),
    }
    
    // Test search across both directories
    println!("\nSearching for 'allowed_dirs' across both directories...");
    let search_result = client.call_tool("search", &json!({
        "root_path": current_dir.to_str().unwrap(),
        "pattern": "allowed_dirs",
        "recursive": true,
        "max_results": 5
    })).await?;
    
    match &search_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            // Show the first 400 characters of the search results
            let preview: String = text.chars().take(400).collect();
            println!("Search result (first 400 chars):\n{}", preview);
            println!("... (results truncated)");
        },
        _ => println!("Unexpected content type"),
    }
    
    // Shutdown client
    println!("\nDemo complete, shutting down...");
    client.shutdown().await?;
    
    // Ensure the handler completes
    handler.await?;
    
    Ok(())
}
