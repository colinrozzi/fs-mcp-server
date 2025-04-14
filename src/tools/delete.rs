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
                "description": "Path to delete (full path or relative to one of the allowed directories)"
            },
            "recursive": {
                "type": "boolean",
                "description": "Whether to recursively delete directories",
                "default": false
            },
            "force": {
                "type": "boolean",
                "description": "Force deletion even if errors occur",
                "default": false
            }
        },
        "required": ["path"]
    })
}

// Execute the delete tool
pub fn execute(args: &Value, allowed_paths: &AllowedPaths) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path_str = args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path parameter"))?;
    
    // Extract optional parameters
    let recursive = args.get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let force = args.get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    debug!(
        "Deleting path: '{}', recursive: {}, force: {}",
        path_str, recursive, force
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
                    format!("Path not found: '{}'", path_str),
                PathError::IoError(io_err) => 
                    format!("IO error: {}", io_err),
            };
            
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text { text: error_message }],
                is_error: Some(true),
            });
        }
    };
    
    // Check if the path exists
    if !validated_path.exists() {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: format!("Path does not exist: '{}'", path_str),
            }],
            is_error: Some(true),
        });
    }
    
    // Determine if it's a file or directory
    let is_dir = validated_path.is_dir();
    let relative_path = allowed_paths.closest_relative_path(&validated_path);
    
    // Delete the path
    let result = if is_dir {
        if recursive {
            fs::remove_dir_all(&validated_path)
        } else {
            fs::remove_dir(&validated_path)
        }
    } else {
        fs::remove_file(&validated_path)
    };
    
    // Handle the result
    match result {
        Ok(_) => {
            let item_type = if is_dir { "directory" } else { "file" };
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Deleted {}: '{}'", item_type, relative_path),
                }],
                is_error: Some(false),
            })
        }
        Err(e) => {
            // If force is enabled, return success with a warning
            if force {
                let item_type = if is_dir { "directory" } else { "file" };
                Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!("Deletion completed with warning: {} (path: '{}')", e, relative_path),
                    }],
                    is_error: Some(false),
                })
            } else {
                Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!("Failed to delete path: {}", e),
                    }],
                    is_error: Some(true),
                })
            }
        }
    }
}
