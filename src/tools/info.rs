use anyhow::{anyhow, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    time::SystemTime,
};
use tracing::debug;
use chrono::{DateTime, Utc};

use crate::utils::path::{AllowedPaths, PathError};

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to get information for (full path or relative to one of the allowed directories)"
            }
        },
        "required": ["path"]
    })
}

// Execute the info tool
pub fn execute(args: &Value, allowed_paths: &AllowedPaths) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path_str = args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path parameter"))?;
    
    debug!("Getting info for path: '{}'", path_str);
    
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
    
    // Determine the file type
    let file_type = if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else if metadata.is_symlink() {
        "symlink"
    } else {
        "unknown"
    };
    
    // Extract the file name
    let name = match validated_path.file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => {
            if validated_path.to_string_lossy().ends_with('/') || 
               validated_path.to_string_lossy().ends_with('\\') {
                // Root directory or directory with no name
                ".".to_string()
            } else {
                // Some other path with no file name
                path_str.to_string()
            }
        }
    };
    
    // Format timestamps
    let created_time = metadata.created().ok().and_then(|time| {
        system_time_to_iso8601(time).ok()
    });
    
    let modified_time = metadata.modified().ok().and_then(|time| {
        system_time_to_iso8601(time).ok()
    });
    
    let accessed_time = metadata.accessed().ok().and_then(|time| {
        system_time_to_iso8601(time).ok()
    });
    
    // Check permissions
    let readable = metadata.permissions().readonly();
    let writable = !metadata.permissions().readonly();
    // Note: executable is platform-specific and might not be accurate on all systems
    let executable = false;  // Default for simplicity
    
    // Check if the file is hidden
    let is_hidden = name.starts_with('.');
    
    // Get relative path
    let relative_path = allowed_paths.closest_relative_path(&validated_path);
    
    // Build the result
    let result = json!({
        "exists": true,
        "type": file_type,
        "name": name,
        "path": relative_path,
        "size": if metadata.is_file() { metadata.len() } else { 0 },
        "created": created_time,
        "modified": modified_time,
        "accessed": accessed_time,
        "permissions": {
            "readable": readable,
            "writable": writable,
            "executable": executable
        },
        "is_hidden": is_hidden
    });
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Text {
            text: result.to_string(),
        }],
        is_error: Some(false),
    })
}

// Helper function to convert SystemTime to ISO 8601 format
fn system_time_to_iso8601(time: SystemTime) -> Result<String> {
    let datetime = DateTime::<Utc>::from(time);
    Ok(datetime.to_rfc3339())
}
