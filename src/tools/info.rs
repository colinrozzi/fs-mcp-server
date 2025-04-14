use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    os::unix::fs::MetadataExt,
    os::unix::fs::PermissionsExt,
};
use chrono::{DateTime, Utc};
use tracing::debug;

use crate::utils::path::{validate_path, PathError};

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to get information for (relative to server root)"
            }
        },
        "required": ["path"]
    })
}

// Execute the info tool
pub fn execute(args: &Value, server_root: &Path) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path = args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path parameter"))?;
    
    debug!("Getting info for: '{}'", path);
    
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
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Path not found: '{}'", path),
                }],
                is_error: Some(true),
            });
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
    
    // Get metadata
    let metadata = match fs::metadata(&validated_path) {
        Ok(m) => m,
        Err(e) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Failed to get metadata: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Determine file type
    let file_type = if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else if metadata.is_symlink() {
        "symlink"
    } else {
        "unknown"
    };
    
    // Get file name
    let name = validated_path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());
    
    // Get size (0 for directories)
    let size = if metadata.is_file() {
        metadata.len()
    } else {
        0
    };
    
    // Get timestamps
    let created = metadata.created().ok()
        .map(|t| format_time(t));
    
    let modified = metadata.modified().ok()
        .map(|t| format_time(t));
    
    let accessed = metadata.accessed().ok()
        .map(|t| format_time(t));
    
    // Get permissions
    let permissions = metadata.permissions();
    let mode = permissions.mode();
    
    let readable = (mode & 0o400) != 0;
    let writable = (mode & 0o200) != 0;
    let executable = (mode & 0o100) != 0;
    
    // Check if hidden (starts with .)
    let is_hidden = name.starts_with('.');
    
    // Format the output
    let mut result = String::new();
    
    result.push_str(&format!("File Information: '{}'\n\n", path));
    result.push_str(&format!("Name: {}\n", name));
    result.push_str(&format!("Type: {}\n", file_type));
    result.push_str(&format!("Size: {} bytes\n", size));
    
    if let Some(t) = created {
        result.push_str(&format!("Created: {}\n", t));
    }
    
    if let Some(t) = modified {
        result.push_str(&format!("Modified: {}\n", t));
    }
    
    if let Some(t) = accessed {
        result.push_str(&format!("Accessed: {}\n", t));
    }
    
    result.push_str("\nPermissions:\n");
    result.push_str(&format!("  Readable: {}\n", readable));
    result.push_str(&format!("  Writable: {}\n", writable));
    result.push_str(&format!("  Executable: {}\n", executable));
    result.push_str(&format!("  Mode: {:o}\n", mode & 0o777));
    
    result.push_str(&format!("\nHidden: {}\n", is_hidden));
    
    if metadata.is_dir() {
        // Count entries for directories
        match fs::read_dir(&validated_path) {
            Ok(entries) => {
                let count = entries.count();
                result.push_str(&format!("Contents: {} items\n", count));
            }
            Err(e) => {
                result.push_str(&format!("Error reading directory contents: {}\n", e));
            }
        }
    }
    
    // Additional Unix-specific information
    result.push_str("\nAdditional Info:\n");
    result.push_str(&format!("  Device: {}\n", metadata.dev()));
    result.push_str(&format!("  Inode: {}\n", metadata.ino()));
    result.push_str(&format!("  User ID: {}\n", metadata.uid()));
    result.push_str(&format!("  Group ID: {}\n", metadata.gid()));
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Text {
            text: result,
        }],
        is_error: Some(false),
    })
}

// Helper function to format timestamps
fn format_time(time: std::time::SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    datetime.to_rfc3339()
}
