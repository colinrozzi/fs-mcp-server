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
    
    // Create a temporary test directory
    let temp_dir = tempfile::tempdir()?;
    let temp_dir_path = temp_dir.path().to_str().unwrap().to_string();
    
    println!("Using temporary directory: {}", temp_dir_path);
    
    // Connect to the server - use the temporary directory as the root
    let (transport, mut receiver) = StdioTransport::new(
        server_executable.to_str().unwrap(),
        vec![
            "--root-dir".to_string(),
            temp_dir_path.clone(),
            "--log-level".to_string(),
            "debug".to_string(),
        ],
    );
    
    let client = Arc::new(ClientBuilder::new("enhanced-client", "0.1.0")
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
    
    println!("\n--- RUNNING FILESYSTEM OPERATIONS DEMO ---\n");
    
    // Test 1: Create a directory
    println!("\n1. Creating directory 'test_dir'...");
    let mkdir_result = client.call_tool("mkdir", &json!({
        "path": "test_dir",
        "recursive": true
    })).await?;
    
    print_tool_result(&mkdir_result);
    
    // Test 2: Write a file
    println!("\n2. Writing content to 'test_dir/hello.txt'...");
    let write_result = client.call_tool("write", &json!({
        "path": "test_dir/hello.txt",
        "content": "Hello, world!\nThis is a test file.\nContains searchable pattern.",
        "encoding": "utf8",
        "mode": "create"
    })).await?;
    
    print_tool_result(&write_result);
    
    // Test 3: Write another file
    println!("\n3. Writing content to 'test_dir/data.txt'...");
    let write_result2 = client.call_tool("write", &json!({
        "path": "test_dir/data.txt",
        "content": "This is another file with some data.\nIt also has a searchable pattern inside.",
        "encoding": "utf8",
        "mode": "create"
    })).await?;
    
    print_tool_result(&write_result2);
    
    // Test 4: List files in the directory
    println!("\n4. Listing files in 'test_dir'...");
    let list_result = client.call_tool("list", &json!({
        "path": "test_dir",
        "recursive": false
    })).await?;
    
    print_tool_result(&list_result);
    
    // Test 5: Read file content
    println!("\n5. Reading 'test_dir/hello.txt'...");
    let read_result = client.call_tool("read", &json!({
        "path": "test_dir/hello.txt"
    })).await?;
    
    print_tool_result(&read_result);
    
    // Test 6: Get file info
    println!("\n6. Getting info for 'test_dir/hello.txt'...");
    let info_result = client.call_tool("info", &json!({
        "path": "test_dir/hello.txt"
    })).await?;
    
    print_tool_result(&info_result);
    
    // Test 7: Copy file
    println!("\n7. Copying 'test_dir/hello.txt' to 'test_dir/hello_copy.txt'...");
    let copy_result = client.call_tool("copy", &json!({
        "source": "test_dir/hello.txt",
        "destination": "test_dir/hello_copy.txt"
    })).await?;
    
    print_tool_result(&copy_result);
    
    // Test 8: Create a subdirectory
    println!("\n8. Creating subdirectory 'test_dir/subdir'...");
    let mkdir_subdir_result = client.call_tool("mkdir", &json!({
        "path": "test_dir/subdir"
    })).await?;
    
    print_tool_result(&mkdir_subdir_result);
    
    // Test 9: Move file
    println!("\n9. Moving 'test_dir/hello_copy.txt' to 'test_dir/subdir/moved.txt'...");
    let move_result = client.call_tool("move", &json!({
        "source": "test_dir/hello_copy.txt",
        "destination": "test_dir/subdir/moved.txt"
    })).await?;
    
    print_tool_result(&move_result);
    
    // Test 10: List all files recursively
    println!("\n10. Listing all files recursively...");
    let list_all_result = client.call_tool("list", &json!({
        "path": ".",
        "recursive": true
    })).await?;
    
    print_tool_result(&list_all_result);
    
    // Test 11: Search for pattern
    println!("\n11. Searching for 'searchable pattern'...");
    let search_result = client.call_tool("search", &json!({
        "root_path": ".",
        "pattern": "searchable pattern",
        "recursive": true
    })).await?;
    
    print_tool_result(&search_result);
    
    // Test 12: Delete a file
    println!("\n12. Deleting 'test_dir/data.txt'...");
    let delete_result = client.call_tool("delete", &json!({
        "path": "test_dir/data.txt"
    })).await?;
    
    print_tool_result(&delete_result);
    
    // Test 13: Delete directory recursively
    println!("\n13. Deleting 'test_dir' recursively...");
    let delete_dir_result = client.call_tool("delete", &json!({
        "path": "test_dir",
        "recursive": true
    })).await?;
    
    print_tool_result(&delete_dir_result);
    
    // Final: List everything to show the state
    println!("\nFinal state: Listing all files...");
    let final_list = client.call_tool("list", &json!({
        "path": ".",
        "recursive": true
    })).await?;
    
    print_tool_result(&final_list);
    
    // Shutdown client
    println!("\nDemo complete, shutting down...");
    client.shutdown().await?;
    
    // Ensure the handler completes
    handler.await?;
    
    Ok(())
}

// Helper function to print tool results
fn print_tool_result(result: &mcp_protocol::types::tool::ToolCallResult) {
    match &result.content[0] {
        mcp_protocol::types::tool::ToolContent::Text { text } => {
            // If the text is very long, truncate it
            if text.len() > 500 {
                let preview: String = text.chars().take(500).collect();
                println!("{}\n... (output truncated)", preview);
            } else {
                println!("{}", text);
            }
        }
        _ => println!("Unexpected content type"),
    }
}
