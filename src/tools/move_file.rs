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
            "source": {
                "type": "string",
                "description": "Source path (relative to server root)"
            },
            "destination": {
                "type": "string",
                "description": "Destination path (relative to server root)"
            },
            "overwrite": {
                "type": "boolean",
                "description": "Whether to overwrite existing files",
                "default": false
            }
        },
        "required": ["source", "destination"]
    })
}

// Execute the move tool
pub fn execute(args: &Value, server_root: &Path) -> Result<ToolCallResult> {
    // Extract required parameters
    let source = args.get("source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing source parameter"))?;
    
    let destination = args.get("destination")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing destination parameter"))?;
    
    // Extract optional parameters
    let overwrite = args.get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    debug!(
        "Moving: '{}' to '{}', overwrite: {}",
        source, destination, overwrite
    );
    
    // Validate the source path
    let validated_source = match validate_path(source, server_root) {
        Ok(p) => p,
        Err(PathError::OutsideRoot) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: "Source path is outside of the allowed root directory".to_string(),
                }],
                is_error: Some(true),
            });
        }
        Err(PathError::NotFound) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Source path not found: '{}'", source),
                }],
                is_error: Some(true),
            });
        }
        Err(PathError::IoError(e)) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("IO error with source path: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Validate the destination path
    let validated_destination = match validate_path(destination, server_root) {
        Ok(p) => {
            // If the destination exists
            if p.exists() {
                if !overwrite {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Destination already exists: '{}'. Use overwrite=true to overwrite.", destination),
                        }],
                        is_error: Some(true),
                    });
                }
                
                // For move operations, if destination exists and is a directory,
                // we need to ensure the source is not a directory or the move will fail
                if p.is_dir() && !validated_source.is_dir() {
                    // If destination is a directory, and source is a file, we need to
                    // adjust the destination to include the filename
                    let filename = validated_source.file_name().ok_or_else(|| anyhow!("Invalid source filename"))?;
                    p.join(filename)
                } else {
                    p
                }
            } else {
                p
            }
        }
        Err(PathError::OutsideRoot) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: "Destination path is outside of the allowed root directory".to_string(),
                }],
                is_error: Some(true),
            });
        }
        Err(PathError::NotFound) => {
            // For move operations, NotFound is expected for the destination
            // The parent directory should exist though
            if let Some(parent) = Path::new(destination).parent() {
                let parent_path = server_root.join(parent);
                
                if !parent_path.exists() {
                    // Create parent directories if they don't exist
                    if let Err(e) = fs::create_dir_all(&parent_path) {
                        return Ok(ToolCallResult {
                            content: vec![ToolContent::Text {
                                text: format!("Failed to create parent directories for destination: {}", e),
                            }],
                            is_error: Some(true),
                        });
                    }
                }
            }
            
            server_root.join(destination)
        }
        Err(PathError::IoError(e)) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("IO error with destination path: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Check if source and destination are the same
    if validated_source == validated_destination {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: format!("Source and destination are the same: '{}'", source),
            }],
            is_error: Some(true),
        });
    }
    
    // Remove destination if it exists and overwrite is true
    if validated_destination.exists() && overwrite {
        let result = if validated_destination.is_dir() {
            fs::remove_dir_all(&validated_destination)
        } else {
            fs::remove_file(&validated_destination)
        };
        
        if let Err(e) = result {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Failed to remove existing destination: {}", e),
                }],
                is_error: Some(true),
            });
        }
    }
    
    // Perform the move operation
    match fs::rename(&validated_source, &validated_destination) {
        Ok(_) => {
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Successfully moved '{}' to '{}'", source, destination),
                }],
                is_error: Some(false),
            })
        }
        Err(e) => {
            // Special case for cross-device moves
            if e.kind() == std::io::ErrorKind::CrossesDevices {
                // Try to copy and then delete
                debug!("Cross-device move detected, falling back to copy and delete");
                
                // Copy first
                match copy_and_delete(&validated_source, &validated_destination) {
                    Ok(_) => {
                        Ok(ToolCallResult {
                            content: vec![ToolContent::Text {
                                text: format!("Successfully moved '{}' to '{}' (using copy and delete)", source, destination),
                            }],
                            is_error: Some(false),
                        })
                    }
                    Err(copy_err) => {
                        Ok(ToolCallResult {
                            content: vec![ToolContent::Text {
                                text: format!("Failed to move '{}' to '{}': {}", source, destination, copy_err),
                            }],
                            is_error: Some(true),
                        })
                    }
                }
            } else {
                Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!("Failed to move '{}' to '{}': {}", source, destination, e),
                    }],
                    is_error: Some(true),
                })
            }
        }
    }
}

// Helper function to copy and then delete for cross-device moves
fn copy_and_delete(src: &Path, dest: &Path) -> Result<()> {
    if src.is_dir() {
        // For directories, we need to do a recursive copy
        copy_dir_all(src, dest)?;
        fs::remove_dir_all(src)?;
    } else {
        // For files, a simple copy and delete
        fs::copy(src, dest)?;
        fs::remove_file(src)?;
    }
    
    Ok(())
}

// Helper function to recursively copy a directory
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else if ty.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    
    Ok(())
}
