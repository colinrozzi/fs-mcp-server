use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Write as IoWrite,
    path::{Path, PathBuf},
};
use tracing::{debug, warn};
use base64;

use crate::utils::path::{validate_path, PathError};

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file to write (relative to server root)"
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
pub fn execute(args: &Value, server_root: &Path) -> Result<ToolCallResult> {
    // Extract required parameters
    let path = args.get("path")
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
        "Writing to file: '{}', encoding: '{}', mode: '{}', make_dirs: {}",
        path, encoding, mode, make_dirs
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
            // For write operations, NotFound is expected if creating a new file
            // We still need to validate that its parent directory exists (or can be created)
            match Path::new(path).parent() {
                None => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: "Invalid path: no parent directory".to_string(),
                        }],
                        is_error: Some(true),
                    });
                }
                Some(parent_path) => {
                    let full_parent = server_root.join(parent_path);
                    
                    // Check if we need to create the parent directories
                    if !full_parent.exists() {
                        if make_dirs {
                            if let Err(e) = fs::create_dir_all(&full_parent) {
                                return Ok(ToolCallResult {
                                    content: vec![ToolContent::Text {
                                        text: format!("Failed to create parent directories: {}", e),
                                    }],
                                    is_error: Some(true),
                                });
                            }
                        } else {
                            return Ok(ToolCallResult {
                                content: vec![ToolContent::Text {
                                    text: format!("Parent directory doesn't exist: '{}'. Use make_dirs=true to create it.", parent_path.display()),
                                }],
                                is_error: Some(true),
                            });
                        }
                    }
                    
                    // Use the full path for the write operation
                    server_root.join(path)
                }
            }
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
    
    // Handle different modes
    let file_exists = validated_path.exists();
    
    if file_exists && mode == "create_new" {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: format!("File already exists: '{}'. Cannot create new.", path),
            }],
            is_error: Some(true),
        });
    }
    
    if !file_exists && mode == "append" {
        debug!("File doesn't exist for append mode, creating new file");
    }
    
    // Decode content based on encoding
    let bytes = match encoding {
        "utf8" => content.as_bytes().to_vec(),
        "base64" => {
            match base64::decode(content) {
                Ok(bytes) => bytes,
                Err(e) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Failed to decode base64 content: {}", e),
                        }],
                        is_error: Some(true),
                    });
                }
            }
        },
        _ => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Unsupported encoding: '{}'", encoding),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Create or open the file with appropriate options
    let file_result = match mode {
        "create" | "create_new" => {
            OpenOptions::new()
                .write(true)
                .create(true)
                .create_new(mode == "create_new")
                .truncate(true)
                .open(&validated_path)
        },
        "overwrite" => {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&validated_path)
        },
        "append" => {
            OpenOptions::new()
                .write(true)
                .create(true)
                .append(true)
                .open(&validated_path)
        },
        _ => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Unsupported mode: '{}'", mode),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Write to the file
    let mut file = match file_result {
        Ok(file) => file,
        Err(e) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Failed to open file for writing: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    let write_result = file.write_all(&bytes);
    
    match write_result {
        Ok(_) => {
            // Flush to ensure data is written to disk
            if let Err(e) = file.flush() {
                warn!("Failed to flush file: {}", e);
            }
            
            // Get file metadata
            match file.metadata() {
                Ok(metadata) => {
                    let size = metadata.len();
                    
                    // Success response
                    let result_text = format!(
                        "Successfully wrote {} bytes to '{}'.\nMode: {}\nEncoding: {}",
                        bytes.len(),
                        path,
                        mode,
                        encoding
                    );
                    
                    Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: result_text,
                        }],
                        is_error: Some(false),
                    })
                },
                Err(e) => {
                    warn!("Failed to get file metadata: {}", e);
                    
                    // Still return success, just without size info
                    Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!(
                                "Successfully wrote {} bytes to '{}'.\nMode: {}\nEncoding: {}",
                                bytes.len(),
                                path,
                                mode,
                                encoding
                            ),
                        }],
                        is_error: Some(false),
                    })
                }
            }
        },
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
