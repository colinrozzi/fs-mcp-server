use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};
use tracing::{debug, warn};
use base64;

use crate::utils::path::{AllowedPaths, is_text_file, PathError};

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
                "description": "Path to the file to read (full path or relative to one of the allowed directories)"
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
pub fn execute(args: &Value, allowed_paths: &AllowedPaths, max_file_size: u64) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path_str = args.get("path")
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
        path_str, encoding, start_line, end_line, max_size
    );
    
    // Create Path object
    let path = Path::new(path_str);
    
    // Validate the path
    let validated_path = match allowed_paths.validate_path(path) {
        Ok(p) => p,
        Err(e) => {
            let error_message = match e {
                PathError::OutsideAllowedPaths => 
                    "Path is outside of all allowed directories".to_string(),
                PathError::NotFound => 
                    format!("File not found: '{}'", path_str),
                PathError::IoError(io_err) => 
                    format!("IO error: {}", io_err),
            };
            
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text { text: error_message }],
                is_error: Some(true),
            });
        }
    };
    
    // Check if the path is a file
    if !validated_path.is_file() {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: format!("Path is not a file: '{}'", path_str),
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
        path: path_str.to_string(),
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
                )
            } else {
                // Otherwise read the entire file (up to max_size)
                read_text_file(&validated_path, max_size, file_metadata)
            }
        }
        "base64" | "binary" => {
            read_binary_file(&validated_path, max_size, file_metadata)
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
    
    // Format result text
    let mut result = format!("File: {}\n", metadata.path);
    
    if let Some(modified) = metadata.modified {
        result.push_str(&format!("Modified: {}\n", modified));
    }
    
    result.push_str(&format!("Size: {} bytes\n", metadata.size));
    
    if truncated {
        result.push_str("Note: File was truncated due to size limit\n");
    }
    
    if let Some(start_line) = start_line {
        if let Some(end_line) = end_line {
            result.push_str(&format!("Lines: {}-{} of {}\n", start_line, end_line, line_count));
        } else {
            result.push_str(&format!("Lines: {} to end of {} total\n", start_line, line_count));
        }
    } else if let Some(end_line) = end_line {
        result.push_str(&format!("Lines: 0-{} of {}\n", end_line, line_count));
    } else {
        result.push_str(&format!("Total lines: {}\n", line_count));
    }
    
    result.push_str("\n----- File Content -----\n\n");
    result.push_str(&content);
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Text {
            text: result,
        }],
        is_error: Some(false),
    })
}

// Read a text file up to max_size
fn read_text_file(
    path: &Path,
    max_size: u64,
    metadata: FileMetadata,
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
    
    // Format result text
    let mut result = format!("File: {}\n", metadata.path);
    
    if let Some(modified) = metadata.modified {
        result.push_str(&format!("Modified: {}\n", modified));
    }
    
    result.push_str(&format!("Size: {} bytes\n", metadata.size));
    result.push_str(&format!("Total lines: {}\n", line_count));
    
    if truncated {
        result.push_str("Note: File was truncated due to size limit\n");
    }
    
    result.push_str("\n----- File Content -----\n\n");
    result.push_str(&content);
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Text {
            text: result,
        }],
        is_error: Some(false),
    })
}

// Read a binary file up to max_size and encode as base64
fn read_binary_file(
    path: &Path,
    max_size: u64,
    metadata: FileMetadata,
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
    
    // Format result text
    let mut result = format!("File: {}\n", metadata.path);
    
    if let Some(modified) = metadata.modified {
        result.push_str(&format!("Modified: {}\n", modified));
    }
    
    result.push_str(&format!("Size: {} bytes\n", metadata.size));
    result.push_str(&format!("Bytes read: {}\n", bytes_read));
    result.push_str("Encoding: base64\n");
    
    if truncated {
        result.push_str("Note: File was truncated due to size limit\n");
    }
    
    result.push_str("\n----- Base64 Encoded Content -----\n\n");
    result.push_str(&content);
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Text {
            text: result,
        }],
        is_error: Some(false),
    })
}
