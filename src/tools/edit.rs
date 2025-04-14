use anyhow::{anyhow, Context, Result};
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};
use chrono::{DateTime, Utc};
use std::time::SystemTime;

use crate::utils::path::{AllowedPaths, is_text_file};

// Define operation types
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum EditOperation {
    #[serde(rename = "replace")]
    Replace {
        find: String,
        replace: String,
        #[serde(default)]
        occurrence: i32,
        #[serde(default = "default_case_sensitive")]
        case_sensitive: bool,
    },
    #[serde(rename = "insert")]
    Insert {
        position: usize,
        content: String,
    },
    #[serde(rename = "delete")]
    Delete {
        start: usize,
        end: usize,
    },
    #[serde(rename = "replace_lines")]
    ReplaceLines {
        start_line: usize,
        end_line: usize,
        content: String,
    },
}

// Struct to track operation results
#[derive(Debug, Serialize)]
struct OperationResult {
    operation_index: usize,
    success: bool,
    error: Option<String>,
}

// Struct representing the edit response
#[derive(Debug, Serialize)]
struct EditResponse {
    success: bool,
    path: String,
    operations_applied: usize,
    operations_failed: usize,
    failed_operations: Vec<OperationResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_path: Option<String>,
    metadata: FileMetadata,
}

#[derive(Debug, Serialize)]
struct FileMetadata {
    path: String,
    modified: String,
    size: u64,
}

// Helper function for default value
fn default_case_sensitive() -> bool {
    true
}

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Full path to the file to edit"
            },
            "operations": {
                "type": "array",
                "description": "List of edit operations to perform (in order)",
                "items": {
                    "type": "object",
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "enum": ["replace"],
                                    "description": "Replace operation"
                                },
                                "find": {
                                    "type": "string",
                                    "description": "Text to find (exact match)"
                                },
                                "replace": {
                                    "type": "string",
                                    "description": "Text to insert as replacement"
                                },
                                "occurrence": {
                                    "type": "integer",
                                    "description": "Which occurrence to replace (0-based, -1 for all)",
                                    "default": 0
                                },
                                "case_sensitive": {
                                    "type": "boolean",
                                    "description": "Whether the search is case-sensitive",
                                    "default": true
                                }
                            },
                            "required": ["type", "find", "replace"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "enum": ["insert"],
                                    "description": "Insert operation"
                                },
                                "position": {
                                    "type": "integer",
                                    "description": "Character position to insert at (0-based)"
                                },
                                "content": {
                                    "type": "string",
                                    "description": "Text to insert"
                                }
                            },
                            "required": ["type", "position", "content"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "enum": ["delete"],
                                    "description": "Delete operation"
                                },
                                "start": {
                                    "type": "integer",
                                    "description": "Start character position (0-based, inclusive)"
                                },
                                "end": {
                                    "type": "integer",
                                    "description": "End character position (0-based, exclusive)"
                                }
                            },
                            "required": ["type", "start", "end"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "enum": ["replace_lines"],
                                    "description": "Replace lines operation"
                                },
                                "start_line": {
                                    "type": "integer",
                                    "description": "Start line number (0-based, inclusive)"
                                },
                                "end_line": {
                                    "type": "integer",
                                    "description": "End line number (0-based, inclusive)"
                                },
                                "content": {
                                    "type": "string",
                                    "description": "Text to insert as replacement"
                                }
                            },
                            "required": ["type", "start_line", "end_line", "content"]
                        }
                    ]
                }
            },
            "create_if_missing": {
                "type": "boolean",
                "description": "Create the file if it doesn't exist",
                "default": false
            },
            "backup": {
                "type": "boolean",
                "description": "Create a backup of the original file before editing",
                "default": false
            }
        },
        "required": ["path", "operations"]
    })
}

// Execute the edit tool
pub fn execute(args: &Value, allowed_paths: &AllowedPaths) -> Result<ToolCallResult> {
    // Extract path parameter (required)
    let path_str = args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path parameter"))?;
    
    // Extract operations
    let operations = args.get("operations")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing or invalid operations array"))?;
    
    if operations.is_empty() {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: "No edit operations specified".to_string(),
            }],
            is_error: Some(true),
        });
    }
    
    // Extract optional parameters
    let create_if_missing = args.get("create_if_missing")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let backup = args.get("backup")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    debug!(
        "Editing file: '{}', operations: {}, create_if_missing: {}, backup: {}",
        path_str, operations.len(), create_if_missing, backup
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
                crate::utils::path::PathError::NotFound => {
                    if create_if_missing {
                        // If the file doesn't exist but we're allowed to create it, check if the parent directory exists
                        if let Some(parent) = path.parent() {
                            match allowed_paths.validate_path(parent) {
                                Ok(_) => {
                                    // Parent directory exists and is allowed, continue with an empty file
                                    path.to_path_buf()
                                },
                                Err(_) => format!("Parent directory not found or not allowed: '{}'", parent.display())
                            }
                        } else {
                            "Invalid path: no parent directory".to_string()
                        }
                    } else {
                        format!("File not found: '{}'. Use create_if_missing=true to create it.", path_str)
                    }
                },
                crate::utils::path::PathError::IoError(io_err) => 
                    format!("IO error: {}", io_err),
            };
            
            if error_message.starts_with("Path is outside") || error_message.starts_with("Parent directory") 
               || error_message.starts_with("Invalid path") {
                return Ok(ToolCallResult {
                    content: vec![ToolContent::Text { text: error_message }],
                    is_error: Some(true),
                });
            }
            
            // For NotFound when create_if_missing is true, we continue with the validated_path
            if !create_if_missing {
                return Ok(ToolCallResult {
                    content: vec![ToolContent::Text { text: error_message }],
                    is_error: Some(true),
                });
            }
        }
    };
    
    // Check if the path is a directory
    if validated_path.is_dir() {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: format!("Path is a directory, not a file: '{}'", path_str),
            }],
            is_error: Some(true),
        });
    }
    
    // Read the file content or create an empty string if it doesn't exist and create_if_missing is true
    let content = if validated_path.exists() {
        // Check if it's a text file
        if !is_text_file(&validated_path)? {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("File appears to be binary, editing not supported: '{}'", path_str),
                }],
                is_error: Some(true),
            });
        }
        
        fs::read_to_string(&validated_path).context("Failed to read file")?
    } else if create_if_missing {
        // Create parent directories if they don't exist
        if let Some(parent) = validated_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).context("Failed to create parent directories")?;
            }
        }
        String::new() // Empty string for new files
    } else {
        return Ok(ToolCallResult {
            content: vec![ToolContent::Text {
                text: format!("File not found: '{}'", path_str),
            }],
            is_error: Some(true),
        });
    };
    
    // Create a backup if requested
    let backup_path = if backup && validated_path.exists() {
        let backup_path_buf = PathBuf::from(format!("{}.bak", validated_path.display()));
        fs::copy(&validated_path, &backup_path_buf).context("Failed to create backup")?;
        Some(backup_path_buf.to_string_lossy().to_string())
    } else {
        None
    };
    
    // Apply operations
    let mut modified_content = content;
    let mut operation_results = Vec::new();
    let mut operations_applied = 0;
    let mut operations_failed = 0;
    
    for (i, op) in operations.iter().enumerate() {
        let operation: EditOperation = match serde_json::from_value(op.clone()) {
            Ok(op) => op,
            Err(e) => {
                warn!("Invalid operation format: {}", e);
                operations_failed += 1;
                operation_results.push(OperationResult {
                    operation_index: i,
                    success: false,
                    error: Some(format!("Invalid operation format: {}", e)),
                });
                continue;
            }
        };
        
        match apply_operation(&operation, &mut modified_content) {
            Ok(_) => {
                operations_applied += 1;
            },
            Err(e) => {
                warn!("Failed to apply operation {}: {}", i, e);
                operations_failed += 1;
                operation_results.push(OperationResult {
                    operation_index: i,
                    success: false,
                    error: Some(format!("Operation failed: {}", e)),
                });
            }
        }
    }
    
    // Only keep failed operations in the results
    let failed_operations = operation_results.into_iter()
        .filter(|r| !r.success)
        .collect::<Vec<_>>();
    
    // Write the modified content back to the file
    fs::write(&validated_path, &modified_content).context("Failed to write modified content")?;
    
    // Get file metadata
    let metadata = fs::metadata(&validated_path).context("Failed to get file metadata")?;
    let size = metadata.len();
    let modified = metadata.modified().unwrap_or_else(|_| SystemTime::now());
    let modified_str = DateTime::<Utc>::from(modified).to_rfc3339();
    
    // Create the response
    let response = EditResponse {
        success: operations_failed == 0,
        path: validated_path.to_string_lossy().to_string(),
        operations_applied,
        operations_failed,
        failed_operations,
        backup_path,
        metadata: FileMetadata {
            path: validated_path.to_string_lossy().to_string(),
            modified: modified_str,
            size,
        },
    };
    
    // Convert to JSON and then to text
    let json = serde_json::to_string_pretty(&response).context("Failed to serialize response")?;
    
    // Build a user-friendly text response
    let mut text = format!("File edited: {}\n", response.path);
    text.push_str(&format!("Operations applied: {}\n", response.operations_applied));
    
    if response.operations_failed > 0 {
        text.push_str(&format!("Operations failed: {}\n", response.operations_failed));
        text.push_str("Failed operations:\n");
        for op in &response.failed_operations {
            text.push_str(&format!("  - Operation {}: {}\n", 
                op.operation_index, 
                op.error.as_deref().unwrap_or("Unknown error")));
        }
    }
    
    if let Some(backup) = &response.backup_path {
        text.push_str(&format!("Backup created: {}\n", backup));
    }
    
    text.push_str(&format!("File size: {} bytes\n", response.metadata.size));
    text.push_str(&format!("Last modified: {}\n", response.metadata.modified));
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Text { text }],
        is_error: Some(operations_failed > 0),
    })
}

// Apply a single operation to the content
fn apply_operation(operation: &EditOperation, content: &mut String) -> Result<()> {
    match operation {
        EditOperation::Replace { find, replace, occurrence, case_sensitive } => {
            if find.is_empty() {
                return Err(anyhow!("Find string cannot be empty"));
            }
            
            let mut replaced = 0;
            
            // For case-insensitive search, we need a custom implementation
            if !case_sensitive {
                let find_lower = find.to_lowercase();
                let mut new_content = String::with_capacity(content.len());
                let mut last_end = 0;
                let mut occurrences = Vec::new();
                
                // Find all occurrences
                let chars: Vec<char> = content.chars().collect();
                let mut i = 0;
                while i <= chars.len().saturating_sub(find.len()) {
                    let slice: String = chars[i..i+find.len()].iter().collect();
                    if slice.to_lowercase() == find_lower {
                        occurrences.push(i);
                        i += find.len();
                    } else {
                        i += 1;
                    }
                }
                
                // Replace the specified occurrences
                let mut done = false;
                for (idx, pos) in occurrences.iter().enumerate() {
                    if *occurrence == -1 || idx == *occurrence as usize {
                        let current_slice = &content[last_end..*pos];
                        new_content.push_str(current_slice);
                        new_content.push_str(replace);
                        last_end = pos + find.len();
                        replaced += 1;
                        if *occurrence != -1 {
                            done = true;
                            break;
                        }
                    }
                }
                
                // Add remaining content
                if !done || *occurrence == -1 {
                    new_content.push_str(&content[last_end..]);
                } else {
                    // For single replacement, we need to add the rest
                    new_content.push_str(&content[last_end..]);
                }
                
                *content = new_content;
            } else {
                // Case-sensitive search is simpler
                if *occurrence == -1 {
                    // Replace all occurrences
                    let new_content = content.replace(find, replace);
                    replaced = if find.is_empty() { 
                        0 
                    } else { 
                        (content.len() - new_content.len()) / find.len() + (new_content.len() - content.len()) / replace.len() 
                    };
                    *content = new_content;
                } else {
                    // Replace a specific occurrence
                    let occurrence_usize = *occurrence as usize;
                    let mut last_end = 0;
                    let mut new_content = String::with_capacity(content.len());
                    let mut found = 0;
                    
                    while let Some(pos) = content[last_end..].find(find) {
                        let absolute_pos = last_end + pos;
                        if found == occurrence_usize {
                            // Found the occurrence we want to replace
                            new_content.push_str(&content[last_end..absolute_pos]);
                            new_content.push_str(replace);
                            last_end = absolute_pos + find.len();
                            replaced = 1;
                            break;
                        } else {
                            // Not the occurrence we want, skip it
                            new_content.push_str(&content[last_end..absolute_pos + find.len()]);
                            last_end = absolute_pos + find.len();
                            found += 1;
                        }
                    }
                    
                    // Add remaining content
                    new_content.push_str(&content[last_end..]);
                    
                    if replaced == 0 {
                        return Err(anyhow!("Occurrence {} of '{}' not found", occurrence, find));
                    }
                    
                    *content = new_content;
                }
            }
            
            if replaced == 0 {
                return Err(anyhow!("Text '{}' not found in file", find));
            }
        },
        EditOperation::Insert { position, content: insert_content } => {
            if *position > content.len() {
                return Err(anyhow!("Insert position {} is beyond the end of the file (length: {})", position, content.len()));
            }
            
            // Split the content at the position and insert the new content
            let (before, after) = content.split_at(*position);
            *content = format!("{}{}{}", before, insert_content, after);
        },
        EditOperation::Delete { start, end } => {
            if *start >= content.len() {
                return Err(anyhow!("Delete start position {} is beyond the end of the file (length: {})", start, content.len()));
            }
            if *end > content.len() {
                return Err(anyhow!("Delete end position {} is beyond the end of the file (length: {})", end, content.len()));
            }
            if start >= end {
                return Err(anyhow!("Delete start position {} must be less than end position {}", start, end));
            }
            
            // Split the content and remove the specified range
            let (before, rest) = content.split_at(*start);
            let after = &rest[(*end - *start)..];
            *content = format!("{}{}", before, after);
        },
        EditOperation::ReplaceLines { start_line, end_line, content: replacement } => {
            if *start_line > *end_line {
                return Err(anyhow!("Start line {} must be less than or equal to end line {}", start_line, end_line));
            }
            
            // Split the content into lines
            let lines: Vec<&str> = content.lines().collect();
            let line_count = lines.len();
            
            if *start_line >= line_count {
                return Err(anyhow!("Start line {} is beyond the end of the file (line count: {})", start_line, line_count));
            }
            
            let effective_end_line = std::cmp::min(*end_line, line_count - 1);
            
            // Rebuild the content with the replacement
            let mut new_content = String::with_capacity(content.len());
            
            // Add lines before the replacement
            for i in 0..*start_line {
                new_content.push_str(lines[i]);
                new_content.push('\n');
            }
            
            // Add the replacement (handling line endings)
            new_content.push_str(replacement);
            if !replacement.ends_with('\n') && effective_end_line < line_count - 1 {
                new_content.push('\n');
            }
            
            // Add lines after the replacement
            for i in (effective_end_line + 1)..line_count {
                new_content.push_str(lines[i]);
                if i < line_count - 1 {
                    new_content.push('\n');
                }
            }
            
            // Special case for the last line with no newline
            if !content.ends_with('\n') && line_count > 0 && effective_end_line == line_count - 1 {
                // Remove the trailing newline we added
                if new_content.ends_with('\n') {
                    new_content.pop();
                }
            }
            
            *content = new_content;
        },
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::NamedTempFile;
    
    #[test]
    fn test_replace_operation() {
        let mut content = String::from("Hello, world! Hello, again!");
        let operation = EditOperation::Replace {
            find: String::from("Hello"),
            replace: String::from("Hi"),
            occurrence: 0,
            case_sensitive: true,
        };
        
        apply_operation(&operation, &mut content).unwrap();
        assert_eq!(content, "Hi, world! Hello, again!");
        
        // Replace all occurrences
        let operation = EditOperation::Replace {
            find: String::from("world"),
            replace: String::from("planet"),
            occurrence: -1,
            case_sensitive: true,
        };
        
        apply_operation(&operation, &mut content).unwrap();
        assert_eq!(content, "Hi, planet! Hello, again!");
    }
    
    #[test]
    fn test_insert_operation() {
        let mut content = String::from("Hello world!");
        let operation = EditOperation::Insert {
            position: 5,
            content: String::from(", beautiful"),
        };
        
        apply_operation(&operation, &mut content).unwrap();
        assert_eq!(content, "Hello, beautiful world!");
    }
    
    #[test]
    fn test_delete_operation() {
        let mut content = String::from("Hello, beautiful world!");
        let operation = EditOperation::Delete {
            start: 5,
            end: 16,
        };
        
        apply_operation(&operation, &mut content).unwrap();
        assert_eq!(content, "Hello world!");
    }
    
    #[test]
    fn test_replace_lines_operation() {
        let mut content = String::from("Line 1\nLine 2\nLine 3\nLine 4");
        let operation = EditOperation::ReplaceLines {
            start_line: 1,
            end_line: 2,
            content: String::from("New Line 2\nNew Line 3"),
        };
        
        apply_operation(&operation, &mut content).unwrap();
        assert_eq!(content, "Line 1\nNew Line 2\nNew Line 3\nLine 4");
    }
}
