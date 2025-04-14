# Filesystem MCP Server

A secure Model Context Protocol (MCP) server implementation for filesystem operations.

## Overview

The Filesystem MCP Server provides a standardized interface for interacting with the local filesystem through the Model Context Protocol. It allows clients to perform common file operations such as listing directories, reading files, and searching file contents in a secure and controlled manner.

## Features

The server provides the following MCP tools:

1. **list**: List files and directories with metadata
2. **read**: Read file contents with various encoding options
3. **write**: Create or update files with content
4. **mkdir**: Create directories
5. **delete**: Delete files or directories
6. **copy**: Copy files or directories
7. **move**: Move or rename files or directories
8. **info**: Get detailed file/directory information
9. **search**: Search file contents (grep-like functionality)
10. **list_allowed_dirs**: List all allowed directories configured on the server
11. **edit**: Perform partial edits on a file without rewriting the entire content

All operations are constrained to configurable allowed directories for security.

## Security

All filesystem operations are constrained to a set of configurable allowed directories. The server validates paths to prevent directory traversal attacks and other security issues. Operations that would access files outside the allowed directories are rejected with appropriate error messages.

## Path Requirements

All file and directory paths provided to the server must be specified as **full absolute paths**. These paths must be located within one of the configured allowed directories to be accessible.

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
# Run with current directory as the allowed directory
./target/release/fs-mcp-server

# Specify allowed directories (comma-separated)
./target/release/fs-mcp-server --allowed-dirs /path/to/dir1,/path/to/dir2

# Use a configuration file listing allowed directories
./target/release/fs-mcp-server --config-file /path/to/config.txt

# Set maximum file size
./target/release/fs-mcp-server --max-file-size 5242880  # 5MB
```

### Configuration File

You can specify allowed directories in a configuration file with one directory per line:

```
# This is a comment
/path/to/directory1
/path/to/directory2
/another/path
```

### Environment Variables

The server can be configured using the following environment variables:

- `FS_ALLOWED_DIRS`: Comma-separated list of allowed directories for filesystem operations
- `FS_CONFIG_FILE`: Path to a configuration file listing allowed directories
- `FS_MAX_FILE_SIZE`: Maximum file size for read operations (in bytes)
- `FS_REQUEST_TIMEOUT`: Request timeout in seconds
- `FS_LOG_LEVEL`: Log level (error, warn, info, debug, trace)
- `FS_LOG_FILE`: Log file path

Example:

```bash
FS_ALLOWED_DIRS=/data,/home/user/docs FS_LOG_LEVEL=debug ./target/release/fs-mcp-server
```

### Protocol Tools

The server provides the following MCP tools:

#### list

Lists files and directories at a specified path.

Parameters:
- `path`: Full path to the directory to list files from
- `pattern`: Optional glob pattern to filter files (default: "*")
- `recursive`: Whether to list files recursively (default: false)
- `include_hidden`: Whether to include hidden files (default: false)
- `metadata`: Whether to include file metadata (default: true)

#### read

Reads file contents with support for different encodings and partial reads.

Parameters:
- `path`: Full path to the file to read
- `encoding`: File encoding (utf8, base64, binary) (default: utf8)
- `start_line`: Start line for partial read (0-indexed)
- `end_line`: End line for partial read (inclusive)
- `max_size`: Maximum number of bytes to read (default: 1MB)

#### search

Searches file contents for matching patterns (grep-like functionality).

Parameters:
- `root_path`: Full path to the root directory to start the search from
- `pattern`: Text pattern to search for in files
- `regex`: Whether to treat pattern as regex (default: false)
- `file_pattern`: Optional glob pattern to filter which files to search (default: "*")
- `recursive`: Whether to search directories recursively (default: true)
- `case_sensitive`: Whether the search should be case-sensitive (default: false)
- `max_results`: Maximum number of results to return (default: 100)
- `max_file_size`: Maximum file size to search (default: 10MB)
- `context_lines`: Number of context lines to include (default: 0)
- `timeout_secs`: Maximum time to spend searching (default: 30s)

#### edit

Performs partial edits on a file without having to rewrite the entire content.

Parameters:
- `path`: Full path to the file to edit
- `operations`: List of edit operations to perform (in order)
  - Replace operation:
    - `type`: "replace"
    - `find`: Text to find (exact match)
    - `replace`: Text to insert as replacement
    - `occurrence`: Which occurrence to replace (0-based, -1 for all) (default: 0)
    - `case_sensitive`: Whether the search is case-sensitive (default: true)
  - Insert operation:
    - `type`: "insert"
    - `position`: Character position to insert at (0-based)
    - `content`: Text to insert
  - Delete operation:
    - `type`: "delete"
    - `start`: Start character position (0-based, inclusive)
    - `end`: End character position (0-based, exclusive)
  - Replace lines operation:
    - `type`: "replace_lines"
    - `start_line`: Start line number (0-based, inclusive)
    - `end_line`: End line number (0-based, inclusive)
    - `content`: Text to insert as replacement
- `create_if_missing`: Create the file if it doesn't exist (default: false)
- `backup`: Create a backup of the original file before editing (default: false)

Example:
```json
{
  "path": "/path/to/file.txt",
  "operations": [
    {
      "type": "replace",
      "find": "hello",
      "replace": "hello world",
      "occurrence": 0,
      "case_sensitive": true
    },
    {
      "type": "insert",
      "position": 100,
      "content": "new text at position 100"
    },
    {
      "type": "replace_lines",
      "start_line": 5,
      "end_line": 7,
      "content": "new line 6\nnew line 7\nnew line 8"
    }
  ],
  "backup": true
}
```

#### list_allowed_dirs

Lists all directories that the server has been configured to allow access to.

Parameters:
- None

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
    
    // List all allowed directories
    let allowed_dirs_result = client.call_tool("list_allowed_dirs", &json!({})).await?;
    println!("Allowed directories: {:?}", allowed_dirs_result);
    
    // List files in a directory
    let list_result = client.call_tool("list", &json!({
        "path": "/path/to/directory"
    })).await?;
    
    // Process result
    println!("List result: {:?}", list_result);
    
    // Edit a file by replacing text
    let edit_result = client.call_tool("edit", &json!({
        "path": "/path/to/file.txt",
        "operations": [
            {
                "type": "replace",
                "find": "TODO",
                "replace": "DONE",
                "occurrence": -1
            }
        ],
        "backup": true
    })).await?;
    
    // Process result
    println!("Edit result: {:?}", edit_result);
    
    // Shutdown
    client.shutdown().await?;
    
    Ok(())
}
```

## Development

### Project Structure

- `src/main.rs`: Server entry point and initialization
- `src/tools/`: Tool implementations (list, read, search, edit, etc.)
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
