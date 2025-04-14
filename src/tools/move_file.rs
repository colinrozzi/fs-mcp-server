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
            "source": {
                "type": "string",
                "description": "Source path (full path or relative to one of the allowed directories)"
            },
            "destination": {
                "type": "string",
                "description": "Destination path (full path or relative to one of the allowed directories)"
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

// Execute the move_file tool
pub fn execute(args: &Value, allowed_paths: &AllowedPaths) -> Result<ToolCallResult> {
    // Extract required parameters
    let source_str = args.get("source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing source parameter"))?;
    
    let destination_str = args.get("destination")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing destination parameter"))?;
    
    // Extract optional parameters
    let overwrite = args.get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    debug!(
        "Moving from '{}' to '{}', overwrite: {}",
        source_str, destination_str, overwrite
    );
    
    // Create Path objects
    let source_path = Path::new(source_str);
    let destination_path = Path::new(destination_str);
    
    // Validate the source path
    let validated_source = match allowed_paths.validate_path(source_path) {
        Ok(p) => p,
        Err(e) => {
            match e {
                PathError::OutsideAllowedPaths => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text { 
                            text: "Source path is outside of all allowed directories".to_string() 
                        }],
                        is_error: Some(true),
                    });
                },
                PathError::NotFound => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text { 
                            text: format!("Source path not found: '{}'", source_str) 
                        }],
                        is_error: Some(true),
                    });
                },
                PathError::IoError(io_err) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text { 
                            text: format!("IO error for source path: {}", io_err) 
                        }],
                        is_error: Some(true),
                    });
                },
            }
        }
    };
    
    // Validate the destination path
    let validated_destination = match allowed_paths.validate_path(destination_path) {
        Ok(p) => p,
        Err(e) => {
            // For destination, NotFound is not necessarily an error,
            // especially if we're creating a new file/directory
            match e {
                PathError::OutsideAllowedPaths => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text { 
                            text: "Destination path is outside of all allowed directories".to_string() 
                        }],
                        is_error: Some(true),
                    });
                },
                PathError::NotFound => {
                    // If the parent exists, we can still move to a new location
                    if let Some(parent) = destination_path.parent() {
                        if parent.exists() {
                            // This is fine, continue with the original path
                            destination_path.to_path_buf()
                        } else {
                            return Ok(ToolCallResult {
                                content: vec![ToolContent::Text { 
                                    text: format!("Destination parent directory not found: '{}'", parent.display()) 
                                }],
                                is_error: Some(true),
                            });
                        }
                    } else {
                        return Ok(ToolCallResult {
                            content: vec![ToolContent::Text { 
                                text: format!("Destination path not found: '{}'", destination_str) 
                            }],
                            is_error: Some(true),
                        });
                    }
                },
                PathError::IoError(io_err) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text { 
                            text: format!("IO error for destination path: {}", io_err) 
                        }],
                        is_error: Some(true),
                    });
                },
            }
        }
    };
    
    // Check if the source exists
    if !validated_source.exists() {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: format!("Source path does not exist: '{}'", source_str),
            }],
            is_error: Some(true),
        });
    }
    
    // Check if the destination exists
    if validated_destination.exists() {
        // Handle directory-to-directory move
        if validated_source.is_dir() && validated_destination.is_dir() {
            let src_name = match validated_source.file_name() {
                Some(name) => name,
                None => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: "Invalid source directory name".to_string(),
                        }],
                        is_error: Some(true),
                    });
                }
            };
            
            let new_dest = validated_destination.join(src_name);
            if new_dest.exists() && !overwrite {
                return Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!("Destination already exists: '{}'", new_dest.display()),
                    }],
                    is_error: Some(true),
                });
            }
            
            // Move directory into destination directory
            match fs::rename(&validated_source, &new_dest) {
                Ok(_) => {
                    let src_rel = allowed_paths.closest_relative_path(&validated_source);
                    let dest_rel = allowed_paths.closest_relative_path(&new_dest);
                    
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Directory moved from '{}' to '{}'", src_rel, dest_rel),
                        }],
                        is_error: Some(false),
                    });
                }
                Err(e) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Failed to move directory: {}", e),
                        }],
                        is_error: Some(true),
                    });
                }
            }
        }
        // Handle file-to-directory move
        else if validated_source.is_file() && validated_destination.is_dir() {
            let src_name = match validated_source.file_name() {
                Some(name) => name,
                None => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: "Invalid source file name".to_string(),
                        }],
                        is_error: Some(true),
                    });
                }
            };
            
            let new_dest = validated_destination.join(src_name);
            if new_dest.exists() && !overwrite {
                return Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!("Destination file already exists: '{}'", new_dest.display()),
                    }],
                    is_error: Some(true),
                });
            }
            
            // Move file into destination directory
            match fs::rename(&validated_source, &new_dest) {
                Ok(_) => {
                    let src_rel = allowed_paths.closest_relative_path(&validated_source);
                    let dest_rel = allowed_paths.closest_relative_path(&new_dest);
                    
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("File moved from '{}' to '{}'", src_rel, dest_rel),
                        }],
                        is_error: Some(false),
                    });
                }
                Err(e) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Failed to move file: {}", e),
                        }],
                        is_error: Some(true),
                    });
                }
            }
        }
        // Direct move with overwrite
        else if overwrite {
            // Delete destination first
            if let Err(e) = fs::remove_file(&validated_destination) {
                if let Err(e) = fs::remove_dir_all(&validated_destination) {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Failed to overwrite destination: {}", e),
                        }],
                        is_error: Some(true),
                    });
                }
            }
        }
        // Direct move without overwrite
        else {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Destination already exists: '{}'", destination_str),
                }],
                is_error: Some(true),
            });
        }
    }
    
    // Perform the move
    match fs::rename(&validated_source, &validated_destination) {
        Ok(_) => {
            let src_rel = allowed_paths.closest_relative_path(&validated_source);
            let dest_rel = allowed_paths.closest_relative_path(&validated_destination);
            
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!(
                        "{} moved from '{}' to '{}'",
                        if validated_source.is_dir() { "Directory" } else { "File" },
                        src_rel, dest_rel
                    ),
                }],
                is_error: Some(false),
            })
        }
        Err(e) => {
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Failed to move: {}", e),
                }],
                is_error: Some(true),
            })
        }
    }
}
