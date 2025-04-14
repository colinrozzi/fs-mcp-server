# Filesystem MCP Server

A secure Model Context Protocol (MCP) server implementation for filesystem operations.

## Overview

The Filesystem MCP Server provides a standardized interface for interacting with the local filesystem through the Model Context Protocol. It allows clients to perform common file operations such as listing directories, reading files, and searching file contents in a secure and controlled manner.

## Features

Currently implemented:
- **File listing**: List files and directories with optional glob pattern filtering
- **File reading**: Read file contents with support for text and binary formats
- **Content searching**: Grep-like functionality for searching text in files

Planned features:
- File writing
- Directory creation
- File deletion
- File copying and moving
- File information retrieval

## Security

All filesystem operations are constrained to a configurable root directory. The server validates paths to prevent directory traversal attacks and other security issues. Operations that would access files outside the root directory are rejected with appropriate error messages.

## Installation

Prerequisites:
- Rust toolchain (1.70.0 or later)
- Cargo package manager

To build the server:

```bash
cargo build --release
```

## Usage

### Running the Server

```bash
# Run with current directory as root
./target/release/fs-mcp-server

# Specify a root directory
./target/release/fs-mcp-server --root-dir /path/to/root

# Set maximum file size
./target/release/fs-mcp-server --max-file-size 5242880  # 5MB
```

### Environment Variables

The server can be configured using the following environment variables:

- `FS_SERVER_ROOT`: Root directory for filesystem operations
- `FS_MAX_FILE_SIZE`: Maximum file size for read operations (in bytes)
- `FS_REQUEST_TIMEOUT`: Request timeout in seconds
- `FS_LOG_LEVEL`: Log level (error, warn, info, debug, trace)
- `FS_LOG_FILE`: Log file path

Example:

```bash
FS_SERVER_ROOT=/data FS_LOG_LEVEL=debug ./target/release/fs-mcp-server
```

### Protocol Tools

The server provides the following MCP tools:

#### fs.list

Lists files and directories at a specified path.

Parameters:
- `path`: Path to list files from (relative to server root)
- `pattern`: Optional glob pattern to filter files (default: "*")
- `recursive`: Whether to list files recursively (default: false)
- `include_hidden`: Whether to include hidden files (default: false)
- `metadata`: Whether to include file metadata (default: true)

#### fs.read

Reads file contents with support for different encodings and partial reads.

Parameters:
- `path`: Path to the file to read (relative to server root)
- `encoding`: File encoding (utf8, base64, binary) (default: utf8)
- `start_line`: Start line for partial read (0-indexed)
- `end_line`: End line for partial read (inclusive)
- `max_size`: Maximum number of bytes to read (default: 1MB)

#### fs.search

Searches file contents for matching patterns (grep-like functionality).

Parameters:
- `root_path`: Root directory to start the search from (relative to server root)
- `pattern`: Text pattern to search for in files
- `regex`: Whether to treat pattern as regex (default: false)
- `file_pattern`: Optional glob pattern to filter which files to search (default: "*")
- `recursive`: Whether to search directories recursively (default: true)
- `case_sensitive`: Whether the search should be case-sensitive (default: false)
- `max_results`: Maximum number of results to return (default: 100)
- `max_file_size`: Maximum file size to search (default: 10MB)
- `context_lines`: Number of context lines to include (default: 0)
- `timeout_secs`: Maximum time to spend searching (default: 30s)

## Client Integration

To use this server with an MCP client:

1. Import the MCP client library for your language
2. Connect to the server using stdio transport
3. Call the available tools using the MCP protocol

Example client code (using the Rust MCP client):

```rust
use mcp_client::{ClientBuilder, transport::StdioTransport};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    // Path to the server executable
    let server_path = "path/to/fs-mcp-server";
    
    // Create and connect to server
    let (transport, mut receiver) = StdioTransport::new(server_path, vec![]);
    
    let client = ClientBuilder::new("fs-client", "0.1.0")
        .with_transport(transport)
        .build()?;
    
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
    
    // List files in a directory
    let list_result = client.call_tool("fs.list", &json!({
        "path": ".",
        "recursive": false
    })).await?;
    
    // Process result
    println!("List result: {:?}", list_result);
    
    // Search for a pattern in files
    let search_result = client.call_tool("fs.search", &json!({
        "root_path": "src",
        "pattern": "TODO",
        "file_pattern": "*.rs"
    })).await?;
    
    // Process result
    println!("Search result: {:?}", search_result);
    
    // Shutdown
    client.shutdown().await?;
    
    Ok(())
}
```

## Development

### Project Structure

- `src/main.rs`: Server entry point and initialization
- `src/tools/`: Tool implementations (list, read, search, etc.)
- `src/utils/`: Utility functions (path validation, etc.)

### Adding New Tools

To add a new filesystem tool:

1. Create a new file in `src/tools/` for your tool
2. Implement the tool's schema and execute functions
3. Add the tool to the server builder in `build_server()` in main.rs

### Building for Different Platforms

```bash
# Build for Linux
cargo build --release --target x86_64-unknown-linux-gnu

# Build for macOS
cargo build --release --target x86_64-apple-darwin

# Build for Windows
cargo build --release --target x86_64-pc-windows-msvc
```

## License

MIT
