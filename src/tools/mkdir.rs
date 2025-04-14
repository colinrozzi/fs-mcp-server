use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
};
use tracing::debug;

use crate::utils::path::{AllowedPaths, PathError};

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the directory to create (full path or relative to one of the allowed directories)"
            },
            "recursive": {
                "type": "boolean",
                "description": "Create parent directories if they don't exist",
                "default": true
            }
        },
        "required": ["path"]
    })
}

// Execute the mkdir tool
pub fn execute(args: &Value, allowed_paths: &AllowedPaths) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path_str = args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path parameter"))?;
    
    // Extract optional parameters
    let recursive = args.get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    
    debug!(
        "Creating directory: '{}', recursive: {}",
        path_str, recursive
    );
    
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
                    // For mkdir, this might be OK if we're creating a new directory.
                    // Just use the path we have
                    path.to_path_buf()
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
    
    // Check if the path already exists
    if validated_path.exists() {
        // If it exists and is a directory, that's fine
        if validated_path.is_dir() {
            let relative_path = allowed_paths.closest_relative_path(&validated_path);
            
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Directory already exists: '{}'", relative_path),
                }],
                is_error: Some(false),
            });
        } else {
            // If it exists but is not a directory, that's an error
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Path exists but is not a directory: '{}'", path_str),
                }],
                is_error: Some(true),
            });
        }
    }
    
    // Create the directory
    let result = if recursive {
        fs::create_dir_all(&validated_path)
    } else {
        fs::create_dir(&validated_path)
    };
    
    // Handle the result
    match result {
        Ok(_) => {
            let relative_path = allowed_paths.closest_relative_path(&validated_path);
            
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Directory created: '{}'", relative_path),
                }],
                is_error: Some(false),
            })
        }
        Err(e) => {
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Failed to create directory: {}", e),
                }],
                is_error: Some(true),
            })
        }
    }
}
