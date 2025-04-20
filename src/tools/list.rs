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
    #[serde(skip_serializing)]
    depth: usize,
    #[serde(skip_serializing)]
    rel_path: String,
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
                "description": "Full path to the directory to list files from"
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
            // Try to get more detailed type information for special files
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                let file_type = entry.file_type();
                if file_type.is_block_device() {
                    "block_device"
                } else if file_type.is_char_device() {
                    "char_device"
                } else if file_type.is_fifo() {
                    "fifo"
                } else if file_type.is_socket() {
                    "socket"
                } else {
                    "unknown"
                }
            }
            #[cfg(not(unix))]
            {
                "unknown"
            }
        };
        
        // Use the full path
        let entry_path = entry.path().to_string_lossy().to_string();
        
        // Get the relative path from the base directory
        let rel_path = if let Ok(rel) = entry.path().strip_prefix(&validated_path) {
            rel.to_string_lossy().to_string()
        } else {
            name.clone()
        };
        
        // Create entry
        let mut result_entry = Entry {
            name,
            path: entry_path,
            entry_type: entry_type.to_string(),
            size: None,
            modified: None,
            is_hidden,
            depth: entry.depth(),
            rel_path,
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
    
    // Define the entry type ordering
    fn get_type_order(entry_type: &str) -> i32 {
        match entry_type {
            "directory" => 0,    // Directories first
            "file" => 1,         // Files second
            "symlink" => 2,       // Symlinks third
            "fifo" => 3,         // FIFOs/named pipes fourth
            "socket" => 4,        // Sockets fifth
            "block_device" => 5,  // Block devices sixth
            "char_device" => 6,   // Character devices seventh
            "unknown" => 7,       // Unknown types last
            _ => 8,               // Any other types after that
        }
    }
    
    // Sort entries by type and then alphabetically by name
    entries.sort_by(|a, b| {
        // First sort by type ordering
        let a_order = get_type_order(&a.entry_type);
        let b_order = get_type_order(&b.entry_type);
        
        // If same type, sort by name
        if a_order == b_order {
            return a.name.cmp(&b.name);
        }
        
        // Otherwise sort by type order
        a_order.cmp(&b_order)
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
            "fifo" => "[FIFO]",
            "socket" => "[SOCK]",
            "block_device" => "[BLK]",
            "char_device" => "[CHR]",
            "unknown" => "[?]",
            _ => "[?]"
        };
        
        // Format with type and name, handling recursive display
        let entry_text = if recursive && entry.depth > 0 {
            // Display relative path for recursive listings
            if !entry.rel_path.is_empty() {
                format!("{} {}", type_str, entry.rel_path)
            } else {
                // Fallback to name with indentation
                let indent = "  ".repeat(entry.depth);
                format!("{}{} {}", indent, type_str, entry.name)
            }
        } else {
            format!("{} {}", type_str, entry.name)
        };
        
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
