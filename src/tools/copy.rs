use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
};
use tracing::debug;
use walkdir::WalkDir;

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
            },
            "recursive": {
                "type": "boolean",
                "description": "Whether to copy directories recursively",
                "default": true
            }
        },
        "required": ["source", "destination"]
    })
}

// Execute the copy tool
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
    
    let recursive = args.get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    
    debug!(
        "Copying: '{}' to '{}', overwrite: {}, recursive: {}",
        source, destination, overwrite, recursive
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
            }
            p
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
            // For copy operations, NotFound is expected for the destination
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
    
    // Check if source is a directory
    let is_dir = validated_source.is_dir();
    
    // Copy based on the type of source
    if is_dir {
        // Directory copy
        if !recursive {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Source is a directory but recursive=false. Cannot copy directory '{}'.", source),
                }],
                is_error: Some(true),
            });
        }
        
        // Create the destination directory if it doesn't exist
        if !validated_destination.exists() {
            if let Err(e) = fs::create_dir_all(&validated_destination) {
                return Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!("Failed to create destination directory: {}", e),
                    }],
                    is_error: Some(true),
                });
            }
        } else if !validated_destination.is_dir() {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Destination exists but is not a directory: '{}'", destination),
                }],
                is_error: Some(true),
            });
        }
        
        // Use walkdir to recursively copy the directory
        let mut total_copied = 0;
        let mut errors = Vec::new();
        
        // Calculate the base path length to create relative paths
        let base_len = validated_source.as_os_str().len();
        
        for entry in WalkDir::new(&validated_source) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    errors.push(format!("Error walking directory: {}", e));
                    continue;
                }
            };
            
            let src_path = entry.path();
            
            // Skip the source directory itself
            if src_path == validated_source {
                continue;
            }
            
            // Create relative path and append to destination
            let relative = &src_path.as_os_str().to_string_lossy()[base_len..];
            let relative = relative.trim_start_matches(std::path::MAIN_SEPARATOR);
            let dest_path = validated_destination.join(relative);
            
            if entry.file_type().is_dir() {
                // Create directory
                if !dest_path.exists() {
                    if let Err(e) = fs::create_dir(&dest_path) {
                        errors.push(format!("Failed to create directory '{}': {}", dest_path.display(), e));
                    }
                }
            } else {
                // Copy file
                let copy_result = copy_file(src_path, &dest_path, overwrite);
                match copy_result {
                    Ok(bytes) => {
                        total_copied += bytes;
                    }
                    Err(e) => {
                        errors.push(format!("Failed to copy '{}' to '{}': {}", src_path.display(), dest_path.display(), e));
                    }
                }
            }
        }
        
        // Generate response
        if errors.is_empty() {
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!(
                        "Successfully copied directory '{}' to '{}'. Total bytes copied: {}",
                        source, destination, total_copied
                    ),
                }],
                is_error: Some(false),
            })
        } else {
            let mut message = format!(
                "Partially copied directory '{}' to '{}' with {} errors:\n",
                source, destination, errors.len()
            );
            
            for (i, error) in errors.iter().enumerate().take(5) {
                message.push_str(&format!("{}. {}\n", i + 1, error));
            }
            
            if errors.len() > 5 {
                message.push_str(&format!("... and {} more errors\n", errors.len() - 5));
            }
            
            message.push_str(&format!("Total bytes copied: {}", total_copied));
            
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: message,
                }],
                is_error: Some(true),
            })
        }
    } else {
        // File copy
        match copy_file(&validated_source, &validated_destination, overwrite) {
            Ok(bytes) => {
                Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!(
                            "Successfully copied file '{}' to '{}'. Bytes copied: {}",
                            source, destination, bytes
                        ),
                    }],
                    is_error: Some(false),
                })
            }
            Err(e) => {
                Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!("Failed to copy file: {}", e),
                    }],
                    is_error: Some(true),
                })
            }
        }
    }
}

// Helper function to copy a single file
fn copy_file(src: &Path, dest: &Path, overwrite: bool) -> Result<u64> {
    // Check if destination exists
    if dest.exists() && !overwrite {
        return Err(anyhow!("Destination exists and overwrite is false"));
    }
    
    // Copy options
    let options = fs::copy(src, dest)?;
    
    Ok(options)
}
