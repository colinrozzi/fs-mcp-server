use anyhow::Result;
use mcp_client::{ClientBuilder, transport::StdioTransport};
use serde_json::json;
use std::env;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Get the path to the compiled server binary
    let server_executable = env::current_dir()?
        .join("target/debug/fs-mcp-server");
    
    println!("Starting server: {:?}", server_executable);
    
    // Connect to the server - use the current directory as the root
    let (transport, mut receiver) = StdioTransport::new(
        server_executable.to_str().unwrap(),
        vec![
            "--root-dir".to_string(),
            env::current_dir()?.to_str().unwrap().to_string(),
            "--log-level".to_string(),
            "debug".to_string(),
        ],
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
    
    // Test fs.list
    println!("\nListing files in current directory...");
    let list_result = client.call_tool("fs.list", &json!({
        "path": ".",
        "recursive": false
    })).await?;
    
    match &list_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            println!("List result:\n{}", text);
        },
        _ => println!("Unexpected content type"),
    }
    
    // Test fs.read on our Cargo.toml file
    println!("\nReading Cargo.toml file...");
    let read_result = client.call_tool("fs.read", &json!({
        "path": "Cargo.toml"
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
    
    // Test fs.search
    println!("\nSearching for 'execute' in src directory...");
    let search_result = client.call_tool("fs.search", &json!({
        "root_path": "src",
        "pattern": "execute",
        "recursive": true
    })).await?;
    
    match &search_result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            // Only show the first 200 characters to avoid flooding the console
            let preview: String = text.chars().take(200).collect();
            println!("Search result (first 200 chars):\n{}", preview);
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
