use anyhow::{anyhow, Result};
use encoding_rs_io::DecodeReaderBytesBuilder;
use glob::Pattern;
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fmt::Write as _,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tracing::{debug, error, warn};
use walkdir::{DirEntry, WalkDir};

use crate::utils::path::{AllowedPaths, is_text_file, PathError};

// Struct representing a search match
#[derive(Debug, Serialize, Deserialize)]
struct Match {
    line_number: usize,
    line: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    context: Vec<Context>,
}

// Struct representing a line of context around a match
#[derive(Debug, Serialize, Deserialize)]
struct Context {
    line_number: usize,
    content: String,
}

// Struct representing matches in a file
#[derive(Debug, Serialize, Deserialize)]
struct FileMatch {
    file: String,
    matches: Vec<Match>,
}

// Struct representing search results
#[derive(Debug, Serialize, Deserialize)]
struct SearchResults {
    total_matches: usize,
    files_searched: usize,
    files_matched: usize,
    matches: Vec<FileMatch>,
}

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root_path": {
                "type": "string",
                "description": "Root directory to start the search from (full path or relative to one of the allowed directories)"
            },
            "pattern": {
                "type": "string",
                "description": "Text pattern to search for in files"
            },
            "regex": {
                "type": "boolean",
                "description": "Whether to treat pattern as regex",
                "default": false
            },
            "file_pattern": {
                "type": "string",
                "description": "Optional glob pattern to filter which files to search",
                "default": "*"
            },
            "recursive": {
                "type": "boolean",
                "description": "Whether to search directories recursively",
                "default": true
            },
            "case_sensitive": {
                "type": "boolean",
                "description": "Whether the search should be case-sensitive",
                "default": false
            },
            "max_results": {
                "type": "integer",
                "description": "Maximum number of results to return",
                "default": 100
            },
            "max_file_size": {
                "type": "integer",
                "description": "Maximum file size to search (in bytes)",
                "default": 10485760
            },
            "context_lines": {
                "type": "integer",
                "description": "Number of context lines to include before and after matches",
                "default": 0
            },
            "timeout_secs": {
                "type": "integer",
                "description": "Maximum time to spend searching (in seconds)",
                "default": 30
            }
        },
        "required": ["root_path", "pattern"]
    })
}

// Execute the search tool
pub fn execute(args: &Value, allowed_paths: &AllowedPaths) -> Result<ToolCallResult> {
    // Extract required parameters
    let root_path_str = args.get("root_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing root_path parameter"))?;
    
    let pattern = args.get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing pattern parameter"))?;
    
    // Extract optional parameters
    let is_regex = args.get("regex")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let file_pattern = args.get("file_pattern")
        .and_then(|v| v.as_str())
        .unwrap_or("*");
    
    let recursive = args.get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    
    let case_sensitive = args.get("case_sensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let max_results = args.get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as usize;
    
    let max_file_size = args.get("max_file_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(10 * 1024 * 1024); // 10MB by default
    
    let context_lines = args.get("context_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    
    let timeout_secs = args.get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30) as u64;
    
    debug!(
        "Searching for '{}' in path '{}', recursive: {}, pattern: {}",
        pattern, root_path_str, recursive, file_pattern
    );
    
    // Create Path object
    let root_path = Path::new(root_path_str);
    
    // Validate the root path
    let validated_path = match allowed_paths.validate_path(root_path) {
        Ok(p) => p,
        Err(e) => {
            let error_message = match e {
                PathError::OutsideAllowedPaths => 
                    "Root path is outside of all allowed directories".to_string(),
                PathError::NotFound => 
                    format!("Root path not found: '{}'", root_path_str),
                PathError::IoError(io_err) => 
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
                text: format!("Path is not a directory: '{}'", root_path_str),
            }],
            is_error: Some(true),
        });
    }
    
    // Create a glob pattern
    let glob_pattern = match Pattern::new(file_pattern) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Invalid file pattern: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };
    
    // Compile the search pattern (regex or literal)
    let regex = if is_regex {
        match RegexBuilder::new(pattern)
            .case_insensitive(!case_sensitive)
            .build() {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Invalid regex pattern: {}", e),
                        }],
                        is_error: Some(true),
                    });
                }
            }
    } else {
        // Escape special characters for literal search
        let escaped_pattern = regex::escape(pattern);
        match RegexBuilder::new(&escaped_pattern)
            .case_insensitive(!case_sensitive)
            .build() {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ToolCallResult {
                        content: vec![ToolContent::Text {
                            text: format!("Failed to create search pattern: {}", e),
                        }],
                        is_error: Some(true),
                    });
                }
            }
    };
    
    // Initialize search state
    let start_time = Instant::now();
    let timeout = Duration::from_secs(timeout_secs);
    
    let mut results = SearchResults {
        total_matches: 0,
        files_searched: 0,
        files_matched: 0,
        matches: Vec::new(),
    };
    
    // Walk the directory
    let walker = WalkDir::new(&validated_path)
        .max_depth(if recursive { usize::MAX } else { 1 })
        .follow_links(false)
        .into_iter();
    
    'outer: for entry_result in walker.filter_entry(|e| should_process_entry(e, &glob_pattern)) {
        // Check timeout
        if start_time.elapsed() > timeout {
            debug!("Search timed out after {} seconds", timeout_secs);
            break;
        }
        
        // Skip errors
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                warn!("Error walking directory: {}", e);
                continue;
            }
        };
        
        // Skip directories
        if entry.file_type().is_dir() {
            continue;
        }
        
        // Get file path relative to allowed directories
        let file_path = allowed_paths.closest_relative_path(entry.path());
        
        // Process file
        results.files_searched += 1;
        
        // Skip files that are too large
        if let Ok(metadata) = entry.metadata() {
            if metadata.len() > max_file_size {
                debug!("Skipping large file: {}", file_path);
                continue;
            }
        }
        
        // Only search text files
        if let Ok(is_text) = is_text_file(entry.path()) {
            if !is_text {
                debug!("Skipping binary file: {}", file_path);
                continue;
            }
        } else {
            debug!("Skipping file (failed to determine if text): {}", file_path);
            continue;
        }
        
        // Search file
        match search_file(entry.path(), &regex, context_lines) {
            Ok(file_matches) => {
                if !file_matches.is_empty() {
                    results.files_matched += 1;
                    results.total_matches += file_matches.len();
                    
                    // Add to results
                    results.matches.push(FileMatch {
                        file: file_path,
                        matches: file_matches,
                    });
                    
                    // Check if we've reached the maximum results
                    if results.total_matches >= max_results {
                        debug!("Reached maximum number of results ({})", max_results);
                        break 'outer;
                    }
                }
            }
            Err(e) => {
                warn!("Error searching file {}: {}", file_path, e);
            }
        }
    }
    
    // Format result text
    let elapsed = start_time.elapsed();
    let mut text = format!(
        "Search results for '{}' in '{}'\n",
        pattern, root_path_str
    );
    writeln!(&mut text, "Results: {}/{} matches in {}/{} files", 
             results.total_matches, 
             results.files_matched,
             results.files_matched,
             results.files_searched)?;
    writeln!(&mut text, "Time: {:.2} seconds", elapsed.as_secs_f64())?;
    
    if results.total_matches > 0 {
        writeln!(&mut text, "\nMatches:")?;
        
        for file_match in &results.matches {
            writeln!(&mut text, "\nFile: {}", file_match.file)?;
            
            for m in &file_match.matches {
                writeln!(&mut text, "  Line {}: {}", m.line_number, m.line.trim())?;
                
                if !m.context.is_empty() {
                    for ctx in &m.context {
                        if ctx.line_number != m.line_number {
                            writeln!(&mut text, "    Line {}: {}", ctx.line_number, ctx.content.trim())?;
                        }
                    }
                }
            }
        }
    } else {
        writeln!(&mut text, "\nNo matches found.")?;
    }
    
    if results.total_matches >= max_results {
        writeln!(&mut text, "\nNote: Maximum result limit reached ({}).", max_results)?;
    }
    
    if elapsed > timeout {
        writeln!(&mut text, "\nNote: Search timed out after {} seconds.", timeout_secs)?;
    }
    
    Ok(ToolCallResult {
        content: vec![ToolContent::Text {
            text,
        }],
        is_error: Some(false),
    })
}

// Determine if an entry should be processed (directory or matching file)
fn should_process_entry(entry: &DirEntry, pattern: &Pattern) -> bool {
    // Always process directories
    if entry.file_type().is_dir() {
        return true;
    }
    
    // Skip hidden files
    let file_name = entry.file_name().to_string_lossy();
    if file_name.starts_with('.') {
        return false;
    }
    
    // Check if the file matches the pattern
    pattern.matches(&file_name)
}

// Search a file for the specified pattern
fn search_file(path: &Path, regex: &Regex, context_lines: usize) -> Result<Vec<Match>> {
    let file = File::open(path)?;
    
    // Use a decoder that handles common text encodings
    let reader = DecodeReaderBytesBuilder::new()
        .encoding(None) // Try to detect encoding
        .utf8_passthru(true)
        .build(BufReader::new(file));
    
    let buffered = BufReader::new(reader);
    
    // Read file line by line
    let mut matches = Vec::new();
    let mut lines = Vec::new();
    
    // Read all lines first for context lookups
    for line_result in buffered.lines() {
        let line = line_result?;
        lines.push(line);
    }
    
    // Process lines
    for (line_num, line) in lines.iter().enumerate() {
        if regex.is_match(line) {
            // Create match with context
            let mut match_context = Vec::new();
            
            // Add context before match
            let start_context = if line_num > context_lines { line_num - context_lines } else { 0 };
            for i in start_context..line_num {
                match_context.push(Context {
                    line_number: i + 1,
                    content: lines[i].clone(),
                });
            }
            
            // Add context after match
            let end_context = std::cmp::min(line_num + context_lines + 1, lines.len());
            for i in line_num+1..end_context {
                match_context.push(Context {
                    line_number: i + 1,
                    content: lines[i].clone(),
                });
            }
            
            matches.push(Match {
                line_number: line_num + 1,
                line: line.clone(),
                context: match_context,
            });
        }
    }
    
    Ok(matches)
}
