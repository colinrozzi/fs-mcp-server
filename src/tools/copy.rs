use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
};
use tracing::debug;
use walkdir::WalkDir;

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
    
    let recursive = args.get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    
    debug!(
        "Copying from '{}' to '{}', overwrite: {}, recursive: {}",
        source_str, destination_str, overwrite, recursive
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
            // especially if we're creating the destination
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
                    // If the parent exists, we can still copy to a new location
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
    
    // Get source metadata
    let source_metadata = match validated_source.metadata() {
        Ok(m) => m,
        Err(e) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Failed to get source metadata: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Perform the copy
    if source_metadata.is_dir() {
        // Directory copy
        if !recursive {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Source is a directory but recursive is false: '{}'", source_str),
                }],
                is_error: Some(true),
            });
        }
        
        // Create destination directory if it doesn't exist
        if !validated_destination.exists() {
            match fs::create_dir_all(&validated_destination) {
                Ok(_) => debug!("Created destination directory: '{}'", validated_destination.display()),
                Err(e) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Failed to create destination directory: {}", e),
                        }],
                        is_error: Some(true),
                    });
                }
            }
        } else if !validated_destination.is_dir() {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Destination exists but is not a directory: '{}'", destination_str),
                }],
                is_error: Some(true),
            });
        }
        
        // Copy all files and subdirectories
        copy_dir_recursive(&validated_source, &validated_destination, overwrite)
    } else {
        // File copy
        copy_file(&validated_source, &validated_destination, overwrite)
    }
}

// Helper function to copy a single file
fn copy_file(source: &Path, destination: &Path, overwrite: bool) -> Result<ToolCallResult> {
    // Check if destination exists and is a file
    if destination.exists() {
        if destination.is_dir() {
            // If destination is a directory, copy the file into it
            let file_name = source.file_name().ok_or_else(|| {
                anyhow!("Invalid source filename")
            })?;
            
            let new_destination = destination.join(file_name);
            return copy_file(source, &new_destination, overwrite);
        } else if !overwrite {
            // If destination exists and overwrite is false, return an error
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!(
                        "Destination file already exists and overwrite is false: '{}'",
                        destination.display()
                    ),
                }],
                is_error: Some(true),
            });
        }
    }
    
    // Copy the file
    match fs::copy(source, destination) {
        Ok(bytes_copied) => {
            Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!(
                        "File copied successfully from '{}' to '{}'. Bytes copied: {}",
                        source.display(), destination.display(), bytes_copied
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

// Helper function to recursively copy a directory
fn copy_dir_recursive(source: &Path, destination: &Path, overwrite: bool) -> Result<ToolCallResult> {
    // Keep track of total bytes copied
    let mut total_bytes_copied: u64 = 0;
    let mut files_copied = 0;
    let mut errors = Vec::new();
    
    // Walk through all items in the source directory
    for entry_result in WalkDir::new(source) {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                errors.push(format!("Error walking directory: {}", e));
                continue;
            }
        };
        
        // Skip the root directory itself
        if entry.path() == source {
            continue;
        }
        
        // Get the relative path from the source root
        let relative_path = entry.path().strip_prefix(source).unwrap();
        let target_path = destination.join(relative_path);
        
        if entry.file_type().is_dir() {
            // Create directories if they don't exist
            if !target_path.exists() {
                match fs::create_dir_all(&target_path) {
                    Ok(_) => debug!("Created directory: '{}'", target_path.display()),
                    Err(e) => {
                        errors.push(format!("Failed to create directory '{}': {}", 
                                            target_path.display(), e));
                    }
                }
            } else if !target_path.is_dir() {
                errors.push(format!("Destination exists but is not a directory: '{}'", 
                                     target_path.display()));
            }
        } else {
            // Copy files
            if target_path.exists() && !overwrite {
                errors.push(format!("File already exists and overwrite is false: '{}'", 
                                    target_path.display()));
                continue;
            }
            
            match fs::copy(entry.path(), &target_path) {
                Ok(bytes) => {
                    total_bytes_copied += bytes;
                    files_copied += 1;
                    debug!("Copied file: '{}' ({} bytes)", target_path.display(), bytes);
                }
                Err(e) => {
                    errors.push(format!("Failed to copy file '{}': {}", 
                                        target_path.display(), e));
                }
            }
        }
    }
    
    // Format response
    let mut message = format!(
        "Directory copied from '{}' to '{}'. Files copied: {}, total bytes: {}",
        source.display(), destination.display(), files_copied, total_bytes_copied
    );
    
    if !errors.is_empty() {
        message.push_str("\n\nWarnings/Errors:");
        for error in &errors {
            message.push_str(&format!("\n- {}", error));
        }
    }
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Text {
            text: message,
        }],
        is_error: Some(!errors.is_empty()),
    })
}
