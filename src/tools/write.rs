use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write as IoWrite,
    path::Path,
};
use tracing::{debug, warn};
use base64;

use crate::utils::path::{AllowedPaths, PathError};

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file to write (full path or relative to one of the allowed directories)"
            },
            "content": {
                "type": "string",
                "description": "Content to write to the file"
            },
            "encoding": {
                "type": "string",
                "description": "Content encoding",
                "enum": ["utf8", "base64"],
                "default": "utf8"
            },
            "mode": {
                "type": "string",
                "description": "Write mode",
                "enum": ["create", "overwrite", "append", "create_new"],
                "default": "overwrite"
            },
            "make_dirs": {
                "type": "boolean",
                "description": "Create parent directories if they don't exist",
                "default": false
            }
        },
        "required": ["path", "content"]
    })
}

// Execute the write tool
pub fn execute(args: &Value, allowed_paths: &AllowedPaths) -> Result<ToolCallResult> {
    // Extract required parameters
    let path_str = args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path parameter"))?;
    
    let content = args.get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing content parameter"))?;
    
    // Extract optional parameters
    let encoding = args.get("encoding")
        .and_then(|v| v.as_str())
        .unwrap_or("utf8");
    
    let mode = args.get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("overwrite");
    
    let make_dirs = args.get("make_dirs")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    debug!(
        "Writing to path: '{}', encoding: '{}', mode: '{}', make_dirs: {}",
        path_str, encoding, mode, make_dirs
    );
    
    // Decode content if needed
    let decoded_content = match encoding {
        "utf8" => content.as_bytes().to_vec(),
        "base64" => {
            match base64::decode(content) {
                Ok(data) => data,
                Err(e) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Failed to decode base64 content: {}", e),
                        }],
                        is_error: Some(true),
                    });
                }
            }
        }
        _ => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Unsupported encoding: '{}'", encoding),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Create Path object
    let path = Path::new(path_str);
    
    // Validate the path
    let validated_path = match allowed_paths.validate_path(path) {
        Ok(p) => p,
        Err(e) => {
            match e {
                PathError::OutsideAllowedPaths => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text { 
                            text: "Path is outside of all allowed directories".to_string() 
                        }],
                        is_error: Some(true),
                    });
                },
                PathError::NotFound => {
                    // For write operations, this might be OK if we're creating a new file
                    // and make_dirs is true.
                    if !make_dirs {
                        return Ok(ToolCallResult {
                            content: vec![ToolContent::Text { 
                                text: format!("Path not found: '{}'", path_str) 
                            }],
                            is_error: Some(true),
                        });
                    } else {
                        // Continue with the path we have
                        path.to_path_buf()
                    }
                },
                PathError::IoError(io_err) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text { 
                            text: format!("IO error: {}", io_err) 
                        }],
                        is_error: Some(true),
                    });
                },
            }
        }
    };
    
    // Create parent directories if needed
    if make_dirs {
        if let Some(parent) = validated_path.parent() {
            if !parent.exists() {
                match fs::create_dir_all(parent) {
                    Ok(_) => {
                        debug!("Created parent directories: '{}'", parent.display());
                    }
                    Err(e) => {
                        return Ok(ToolCallResult {
                            content: vec![ToolContent::Text {
                                text: format!("Failed to create parent directories: {}", e),
                            }],
                            is_error: Some(true),
                        });
                    }
                }
            }
        }
    }
    
    // Determine file open mode and handle existing files
    let file_result = match mode {
        "create" => {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&validated_path)
        }
        "overwrite" => {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&validated_path)
        }
        "append" => {
            OpenOptions::new()
                .write(true)
                .create(true)
                .append(true)
                .open(&validated_path)
        }
        "create_new" => {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&validated_path)
        }
        _ => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Invalid mode: '{}'", mode),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Handle file open result
    let mut file = match file_result {
        Ok(f) => f,
        Err(e) => {
            let error_msg = if mode == "create_new" && e.kind() == std::io::ErrorKind::AlreadyExists {
                format!("File already exists: '{}' and mode is create_new", validated_path.display())
            } else {
                format!("Failed to open file: {}", e)
            };
            
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: error_msg,
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Write content to file
    match file.write_all(&decoded_content) {
        Ok(_) => {
            // Get file metadata
            let metadata = match file.metadata() {
                Ok(m) => m,
                Err(e) => {
                    warn!("Failed to get file metadata: {}", e);
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Content written successfully to '{}' but failed to get metadata: {}", 
                                        validated_path.display(), e),
                        }],
                        is_error: Some(false),
                    });
                }
            };
            
            // Extract modified time if available
            let modified_time = metadata.modified().ok().and_then(|time| {
                chrono::DateTime::<chrono::Utc>::from(time)
                    .to_rfc3339()
                    .into()
            });
            
            // Format success response
            let relative_path = allowed_paths.closest_relative_path(&validated_path);
            
            let response = json!({
                "success": true,
                "path": relative_path,
                "bytes_written": decoded_content.len(),
                "metadata": {
                    "size": metadata.len(),
                    "modified": modified_time
                }
            });
            
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: response.to_string(),
                }],
                is_error: Some(false),
            })
        }
        Err(e) => {
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Failed to write to file: {}", e),
                }],
                is_error: Some(true),
            })
        }
    }
}
