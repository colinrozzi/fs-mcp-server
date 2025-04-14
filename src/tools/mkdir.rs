use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
};
use tracing::debug;

use crate::utils::path::{validate_path, PathError};

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the directory to create (relative to server root)"
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
pub fn execute(args: &Value, server_root: &Path) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path = args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path parameter"))?;
    
    // Extract optional parameters
    let recursive = args.get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    
    debug!(
        "Creating directory: '{}', recursive: {}",
        path, recursive
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
            // For mkdir operations, NotFound is expected
            // We need to create the directory
            server_root.join(path)
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
    
    // Check if the directory already exists
    if validated_path.exists() {
        if validated_path.is_dir() {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Directory already exists: '{}'", path),
                }],
                is_error: Some(false),
            });
        } else {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("A file with the same name already exists: '{}'", path),
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
    
    match result {
        Ok(_) => {
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Successfully created directory: '{}'", path),
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
