use anyhow::{anyhow, Result};
use glob::Pattern;
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use chrono::{DateTime, Utc};
use tracing::{debug, warn};
use walkdir::WalkDir;

use crate::utils::path::AllowedPaths;

// Struct representing a directory entry
#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    name: String,
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    modified: Option<String>,
    is_hidden: bool,
}

// Struct representing list results
#[derive(Debug, Serialize, Deserialize)]
struct ListResults {
    entries: Vec<Entry>,
    directory: String,
}

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to list files from (full path or relative to one of the allowed directories)"
            },
            "pattern": {
                "type": "string",
                "description": "Optional glob pattern to filter files",
                "default": "*"
            },
            "recursive": {
                "type": "boolean",
                "description": "Whether to list files recursively",
                "default": false
            },
            "include_hidden": {
                "type": "boolean",
                "description": "Whether to include hidden files (starting with .)",
                "default": false
            },
            "metadata": {
                "type": "boolean",
                "description": "Whether to include file metadata (size, type, modification time)",
                "default": true
            }
        },
        "required": ["path"]
    })
}

// Execute the list tool
pub fn execute(args: &Value, allowed_paths: &AllowedPaths) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path_str = args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path parameter"))?;
    
    // Extract optional parameters
    let pattern = args.get("pattern")
        .and_then(|v| v.as_str())
        .unwrap_or("*");
    
    let recursive = args.get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let include_hidden = args.get("include_hidden")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let include_metadata = args.get("metadata")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    
    debug!(
        "Listing path: '{}', pattern: '{}', recursive: {}, include_hidden: {}",
        path_str, pattern, recursive, include_hidden
    );
    
    // Create Path object
    let path = Path::new(path_str);
    
    // Validate the path
    let validated_path = match allowed_paths.validate_path(path) {
        Ok(p) => p,
        Err(e) => {
            let error_message = match e {
                crate::utils::path::PathError::OutsideAllowedPaths => 
                    "Path is outside of all allowed directories".to_string(),
                crate::utils::path::PathError::NotFound => 
                    format!("Path not found: '{}'", path_str),
                crate::utils::path::PathError::IoError(io_err) => 
                    format!("IO error: {}", io_err),
            };
            
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text { text: error_message }],
                is_error: Some(true),
            });
        }
    };
    
    // Check if the path is a directory
    if !validated_path.is_dir() {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: format!("Path is not a directory: '{}'", path_str),
            }],
            is_error: Some(true),
        });
    }
    
    // Create a glob pattern
    let glob_pattern = match Pattern::new(pattern) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Invalid pattern: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Collect directory entries
    let mut entries = Vec::new();
    
    // Setup the walker
    let max_depth = if recursive { std::usize::MAX } else { 1 };
    let walker = WalkDir::new(&validated_path)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter();
    
    // Process entries
    for entry_result in walker {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                warn!("Error walking directory: {}", e);
                continue;
            }
        };
        
        // Skip the root directory itself if not at the top level
        if entry.path() == validated_path && entry.depth() > 0 {
            continue;
        }
        
        // Get the file name
        let name = entry.file_name().to_string_lossy().to_string();
        
        // Check if it's a hidden file
        let is_hidden = name.starts_with('.');
        
        // Skip hidden files if not included
        if is_hidden && !include_hidden {
            continue;
        }
        
        // Skip entries that don't match the pattern
        if !glob_pattern.matches(&name) && entry.path() != validated_path {
            continue;
        }
        
        // Get entry type
        let entry_type = if entry.file_type().is_dir() {
            "directory"
        } else if entry.file_type().is_file() {
            "file"
        } else if entry.file_type().is_symlink() {
            "symlink"
        } else {
            "unknown"
        };
        
        // Get the closest relative path from allowed directories
        let entry_path = allowed_paths.closest_relative_path(entry.path());
        
        // Create entry
        let mut result_entry = Entry {
            name,
            path: entry_path,
            entry_type: entry_type.to_string(),
            size: None,
            modified: None,
            is_hidden,
        };
        
        // Add metadata if requested
        if include_metadata {
            // Get file size for files
            if entry.file_type().is_file() {
                if let Ok(metadata) = entry.metadata() {
                    result_entry.size = Some(metadata.len());
                    
                    // Get modification time
                    if let Ok(modified) = metadata.modified() {
                        if let Ok(datetime) = system_time_to_date_string(modified) {
                            result_entry.modified = Some(datetime);
                        }
                    }
                }
            }
        }
        
        entries.push(result_entry);
    }
    
    // Sort entries: directories first, then files, alphabetically
    entries.sort_by(|a, b| {
        match (a.entry_type.as_str(), b.entry_type.as_str()) {
            ("directory", "file") => std::cmp::Ordering::Less,
            ("file", "directory") => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });
    
    // Create the result
    let results = ListResults {
        entries,
        directory: path_str.to_string(),
    };
    
    // Convert to text format
    let mut text = format!("Directory: {}\n\n", results.directory);
    
    for entry in results.entries {
        let type_str = match entry.entry_type.as_str() {
            "directory" => "[DIR]",
            "file" => "[FILE]",
            "symlink" => "[LINK]",
            _ => "[?]"
        };
        
        // Format with type and name
        let entry_text = format!("{} {}", type_str, entry.name);
        
        // Add size if available
        let entry_with_size = if let Some(size) = entry.size {
            format!("{} ({} bytes)", entry_text, size)
        } else {
            entry_text
        };
        
        writeln!(&mut text, "{}", entry_with_size)?;
    }
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Text {
            text,
        }],
        is_error: Some(false),
    })
}

// Helper function to convert SystemTime to formatted date string
fn system_time_to_date_string(time: SystemTime) -> Result<String, std::time::SystemTimeError> {
    let datetime = DateTime::<Utc>::from(time);
    Ok(datetime.to_rfc3339())
}
