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
                "description": "Path to delete (relative to server root)"
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
pub fn execute(args: &Value, server_root: &Path) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path = args.get("path")
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
        "Deleting: '{}', recursive: {}, force: {}",
        path, recursive, force
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
            // If force is true, ignore not found errors
            if force {
                return Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!("Path not found: '{}' (ignored due to force flag)", path),
                    }],
                    is_error: Some(false),
                });
            } else {
                return Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!("Path not found: '{}'", path),
                    }],
                    is_error: Some(true),
                });
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
    
    // Check if it's a directory
    let is_dir = validated_path.is_dir();
    
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
            let type_str = if is_dir { "directory" } else { "file" };
            
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Successfully deleted {}: '{}'", type_str, path),
                }],
                is_error: Some(false),
            })
        }
        Err(e) => {
            // If force is true, don't report errors as errors
            if force {
                Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!(
                            "Failed to delete '{}': {} (ignored due to force flag)",
                            path, e
                        ),
                    }],
                    is_error: Some(false),
                })
            } else {
                // For non-recursive deletion of non-empty directories, provide a more helpful message
                if is_dir && !recursive && e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                    Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!(
                                "Directory not empty: '{}'. Use recursive=true to delete non-empty directories.",
                                path
                            ),
                        }],
                        is_error: Some(true),
                    })
                } else {
                    Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Failed to delete '{}': {}", path, e),
                        }],
                        is_error: Some(true),
                    })
                }
            }
        }
    }
}
