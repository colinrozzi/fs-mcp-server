use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
};
use tracing::{debug, warn};

use crate::utils::path::{is_text_file, validate_path, PathError};

// Struct representing file read results
#[derive(Debug, Serialize, Deserialize)]
struct ReadResult {
    content: String,
    encoding: String,
    size: u64,
    truncated: bool,
    line_count: Option<usize>,
    metadata: FileMetadata,
}

// Struct representing file metadata
#[derive(Debug, Serialize, Deserialize)]
struct FileMetadata {
    path: String,
    modified: Option<String>,
    size: u64,
}

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file to read (relative to server root)"
            },
            "encoding": {
                "type": "string",
                "description": "File encoding",
                "enum": ["utf8", "base64", "binary"],
                "default": "utf8"
            },
            "start_line": {
                "type": "integer",
                "description": "Start line for partial read (0-indexed)"
            },
            "end_line": {
                "type": "integer",
                "description": "End line for partial read (inclusive)"
            },
            "max_size": {
                "type": "integer",
                "description": "Maximum number of bytes to read",
                "default": 1048576
            }
        },
        "required": ["path"]
    })
}

// Execute the read tool
pub fn execute(args: &Value, server_root: &Path, max_file_size: u64) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path = args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path parameter"))?;
    
    // Extract optional parameters
    let encoding = args.get("encoding")
        .and_then(|v| v.as_str())
        .unwrap_or("utf8");
    
    let start_line = args.get("start_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    
    let end_line = args.get("end_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    
    let user_max_size = args.get("max_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(1048576); // Default to 1MB
    
    // Use the smaller of user-specified and server-configured max size
    let max_size = std::cmp::min(user_max_size, max_file_size);
    
    debug!(
        "Reading file: '{}', encoding: '{}', start_line: {:?}, end_line: {:?}, max_size: {}",
        path, encoding, start_line, end_line, max_size
    );
    
    // Validate the path
    let validated_path = match validate_path(path, server_root) {
        Ok(p) => p,
        Err(PathError::OutsideRoot) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: "Path is outside of the allowed root directory".to_string(),
                }],
                is_error: Some(true),
            });
        }
        Err(PathError::NotFound) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("File not found: '{}'", path),
                }],
                is_error: Some(true),
            });
        }
        Err(PathError::IoError(e)) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("IO error: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Check if the path is a file
    if !validated_path.is_file() {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: format!("Path is not a file: '{}'", path),
            }],
            is_error: Some(true),
        });
    }
    
    // Get file metadata
    let metadata = match validated_path.metadata() {
        Ok(m) => m,
        Err(e) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Failed to get file metadata: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Check file size
    let file_size = metadata.len();
    if file_size > max_size {
        warn!("File size {} exceeds maximum allowed size {}", file_size, max_size);
    }
    
    // Determine if this is a text or binary file
    let is_text = match is_text_file(&validated_path) {
        Ok(is_text) => is_text,
        Err(e) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Failed to determine file type: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // For binary files, enforce base64 encoding
    let actual_encoding = if !is_text && encoding == "utf8" {
        warn!("Binary file detected, forcing base64 encoding");
        "base64"
    } else {
        encoding
    };
    
    // Extract modified time if available
    let modified_time = metadata.modified().ok().and_then(|time| {
        chrono::DateTime::<chrono::Utc>::from(time)
            .to_rfc3339()
            .into()
    });
    
    // Prepare metadata structure
    let file_metadata = FileMetadata {
        path: path.to_string(),
        modified: modified_time,
        size: file_size,
    };
    
    // Handle different read modes
    match actual_encoding {
        "utf8" => {
            // If line range is specified, use line-based reading
            if start_line.is_some() || end_line.is_some() {
                read_text_lines(
                    &validated_path,
                    start_line,
                    end_line,
                    max_size,
                    file_metadata,
                    server_root,
                )
            } else {
                // Otherwise read the entire file (up to max_size)
                read_text_file(&validated_path, max_size, file_metadata, server_root)
            }
        }
        "base64" | "binary" => {
            read_binary_file(&validated_path, max_size, file_metadata, server_root)
        }
        _ => {
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Unsupported encoding: '{}'", actual_encoding),
                }],
                is_error: Some(true),
            })
        }
    }
}

// Read a text file line by line
fn read_text_lines(
    path: &Path,
    start_line: Option<usize>,
    end_line: Option<usize>,
    max_size: u64,
    metadata: FileMetadata,
    server_root: &Path,
) -> Result<ToolCallResult> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    
    let start = start_line.unwrap_or(0);
    let end = end_line.unwrap_or(usize::MAX);
    
    let mut content = String::new();
    let mut line_count = 0;
    let mut byte_count = 0;
    let mut truncated = false;
    
    // Read lines and filter based on range
    for (i, line_result) in reader.lines().enumerate() {
        line_count += 1;
        
        // Skip lines before start
        if i < start {
            continue;
        }
        
        // Stop after end line
        if i > end {
            break;
        }
        
        // Read the line
        let line = line_result?;
        
        // Check if adding this line would exceed max_size
        let line_bytes = line.len() + 1; // +1 for newline
        if byte_count + line_bytes as u64 > max_size {
            truncated = true;
            break;
        }
        
        // Add line to content
        content.push_str(&line);
        content.push('\n');
        byte_count += line_bytes as u64;
    }
    
    // Create result object
    let result = ReadResult {
        content,
        encoding: "utf8".to_string(),
        size: byte_count,
        truncated,
        line_count: Some(line_count),
        metadata,
    };
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Json {
            json: serde_json::to_value(result)?,
        }],
        is_error: Some(false),
    })
}

// Read a text file up to max_size
fn read_text_file(
    path: &Path,
    max_size: u64,
    metadata: FileMetadata,
    server_root: &Path,
) -> Result<ToolCallResult> {
    let mut file = File::open(path)?;
    
    // Determine how much to read
    let file_size = metadata.size;
    let bytes_to_read = std::cmp::min(file_size, max_size) as usize;
    let truncated = file_size > max_size;
    
    // Read file content
    let mut content = String::with_capacity(bytes_to_read);
    file.take(max_size).read_to_string(&mut content)?;
    
    // Count lines
    let line_count = content.lines().count();
    
    // Create result object
    let result = ReadResult {
        content,
        encoding: "utf8".to_string(),
        size: bytes_to_read as u64,
        truncated,
        line_count: Some(line_count),
        metadata,
    };
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Json {
            json: serde_json::to_value(result)?,
        }],
        is_error: Some(false),
    })
}

// Read a binary file up to max_size and encode as base64
fn read_binary_file(
    path: &Path,
    max_size: u64,
    metadata: FileMetadata,
    server_root: &Path,
) -> Result<ToolCallResult> {
    let mut file = File::open(path)?;
    
    // Determine how much to read
    let file_size = metadata.size;
    let bytes_to_read = std::cmp::min(file_size, max_size) as usize;
    let truncated = file_size > max_size;
    
    // Read file content
    let mut buffer = vec![0; bytes_to_read];
    let bytes_read = file.read(&mut buffer)?;
    buffer.truncate(bytes_read);
    
    // Encode as base64
    let content = base64::encode(&buffer);
    
    // Create result object
    let result = ReadResult {
        content,
        encoding: "base64".to_string(),
        size: bytes_read as u64,
        truncated,
        line_count: None, // Line count not applicable for binary files
        metadata,
    };
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Json {
            json: serde_json::to_value(result)?,
        }],
        is_error: Some(false),
    })
}
