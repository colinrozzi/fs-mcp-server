use anyhow::Result;
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use serde_json::{json, Value};

use crate::utils::path::AllowedPaths;

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}

// Execute the list_allowed_dirs tool
pub fn execute(_args: &Value, allowed_paths: &AllowedPaths) -> Result<ToolCallResult> {
    // Get all allowed directories
    let dirs = allowed_paths.all_paths();
    
    // Create the result text
    let mut text = format!("Allowed directories ({})\n\n", dirs.len());
    
    // List each directory
    for (i, dir) in dirs.iter().enumerate() {
        text.push_str(&format!("{}. {}\n", i + 1, dir.display()));
    }
    
    // Add note about using full paths
    text.push_str("\nNote: All file and directory paths in requests must be specified as full paths. ");
    text.push_str("Paths must be within one of these allowed directories to be accessible.\n");
    
    // Return the result
    Ok(ToolCallResult {
        content: vec![ToolContent::Text {
            text,
        }],
        is_error: Some(false),
    })
}
