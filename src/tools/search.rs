use anyhow::{anyhow, Result};
use encoding_rs_io::DecodeReaderBytesBuilder;
use glob::Pattern;
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fmt::Write as _,
    fs::{self, File, Metadata},
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinSet};
use tracing::{debug, error, info, warn};
use walkdir::{DirEntry, WalkDir};

use crate::utils::path::{validate_path, PathError};

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

// Struct for representing search parameters
#[derive(Debug, Deserialize)]
struct SearchParams {
    root_path: String,
    pattern: String,
    #[serde(default = "default_file_pattern")]
    file_pattern: String,
    #[serde(default = "default_true")]
    recursive: bool,
    #[serde(default = "default_false")]
    case_sensitive: bool,
    #[serde(default = "default_false")]
    regex: bool,
    #[serde(default = "default_max_results")]
    max_results: usize,
    #[serde(default = "default_max_file_size")]
    max_file_size: u64,
    #[serde(default = "default_zero")]
    context_lines: usize,
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
}

// Default values for optional parameters
fn default_file_pattern() -> String {
    "*".to_string()
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_zero() -> usize {
    0
}

fn default_max_results() -> usize {
    100
}

fn default_max_file_size() -> u64 {
    10 * 1024 * 1024 // 10MB
}

fn default_timeout() -> u64 {
    30 // 30 seconds
}

// Check if a file is likely binary based on content sampling
fn is_likely_binary(path: &Path, max_check_size: usize) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut buffer = vec![0; max_check_size.min(8192)]; // Check at most 8KB
    let bytes_read = file.read(&mut buffer)?;
    buffer.truncate(bytes_read);

    // Check for null bytes and other binary indicators
    Ok(buffer.iter().take(bytes_read).any(|&b| b == 0))
}

// Extract context lines around a match
fn extract_context(
    lines: &[String],
    line_idx: usize,
    context_lines: usize,
) -> Vec<Context> {
    if context_lines == 0 {
        return Vec::new();
    }

    let start = line_idx.saturating_sub(context_lines);
    let end = (line_idx + context_lines).min(lines.len() - 1);
    
    let mut context = Vec::with_capacity((end - start + 1).saturating_sub(1));
    
    // Add lines before the match
    for i in start..line_idx {
        context.push(Context {
            line_number: i + 1,
            content: lines[i].clone(),
        });
    }
    
    // Add lines after the match
    for i in (line_idx + 1)..=end {
        context.push(Context {
            line_number: i + 1,
            content: lines[i].clone(),
        });
    }
    
    context
}

// Check if an entry should be processed based on file pattern and size
fn should_process_entry(
    entry: &DirEntry,
    file_pattern: &Pattern,
    max_file_size: u64,
) -> Result<bool> {
    let path = entry.path();
    
    // Skip directories
    if !entry.file_type().is_file() {
        return Ok(false);
    }

    // Check if file name matches pattern
    let file_name = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    
    if !file_pattern.matches(file_name) {
        return Ok(false);
    }
    
    // Check file size
    match entry.metadata() {
        Ok(metadata) => {
            if metadata.len() > max_file_size {
                debug!("Skipping large file {}: {} bytes", path.display(), metadata.len());
                return Ok(false);
            }
        }
        Err(err) => {
            warn!("Could not get metadata for {}: {}", path.display(), err);
            return Ok(false);
        }
    }
    
    // Check if it's a binary file
    match is_likely_binary(path, 4096) {
        Ok(true) => {
            debug!("Skipping binary file: {}", path.display());
            return Ok(false);
        }
        Ok(false) => Ok(true),
        Err(err) => {
            warn!("Error checking if file is binary {}: {}", path.display(), err);
            Ok(false)
        }
    }
}

// Process a single file looking for matches
fn process_file(
    path: &Path,
    regex: &Regex,
    context_lines: usize,
    server_root: &Path,
) -> Result<Option<FileMatch>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) => {
            warn!("Could not open file {}: {}", path.display(), err);
            return Ok(None);
        }
    };
    
    // Use a decoder that handles various text encodings
    let decoder = DecodeReaderBytesBuilder::new()
        .encoding(None)
        .utf8_passthru(true)
        .build(file);
    
    let reader = BufReader::new(decoder);
    let lines: Vec<String> = reader.lines()
        .filter_map(Result::ok)
        .collect();
    
    let mut file_matches = Vec::new();
    
    for (line_idx, line) in lines.iter().enumerate() {
        if regex.is_match(line) {
            let context = extract_context(&lines, line_idx, context_lines);
            
            file_matches.push(Match {
                line_number: line_idx + 1,
                line: line.clone(),
                context,
            });
        }
    }
    
    if file_matches.is_empty() {
        return Ok(None);
    }
    
    // Convert absolute path to path relative to server root
    let relative_path = path.strip_prefix(server_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    
    Ok(Some(FileMatch {
        file: relative_path,
        matches: file_matches,
    }))
}

// Worker function for processing files in parallel
async fn search_worker(
    receiver: mpsc::Receiver<PathBuf>,
    regex: Arc<Regex>,
    context_lines: usize,
    server_root: Arc<Path>,
) -> Vec<FileMatch> {
    let mut results = Vec::new();
    
    while let Some(path) = receiver.recv().await {
        match process_file(&path, &regex, context_lines, &server_root) {
            Ok(Some(file_match)) => {
                results.push(file_match);
            }
            Ok(None) => {}
            Err(err) => {
                error!("Error processing file {}: {}", path.display(), err);
            }
        }
    }
    
    results
}

// Main search function
async fn search_files(
    params: &SearchParams,
    server_root: &Path,
) -> Result<SearchResults> {
    // Validate the path
    let root_path = match validate_path(&params.root_path, server_root) {
        Ok(path) => path,
        Err(PathError::OutsideRoot) => {
            return Err(anyhow!("Path is outside of the allowed root directory"));
        }
        Err(PathError::NotFound) => {
            return Err(anyhow!("The specified path does not exist"));
        }
        Err(PathError::IoError(err)) => {
            return Err(anyhow!("IO error: {}", err));
        }
    };
    
    // Prepare regex for searching
    let regex_builder = if params.regex {
        RegexBuilder::new(&params.pattern)
    } else {
        RegexBuilder::new(&regex::escape(&params.pattern))
    };
    
    let regex = Arc::new(
        regex_builder
            .case_insensitive(!params.case_sensitive)
            .build()
            .map_err(|e| anyhow!("Invalid regex pattern: {}", e))?,
    );
    
    // Prepare file pattern
    let file_pattern = Pattern::new(&params.file_pattern)
        .map_err(|e| anyhow!("Invalid file pattern: {}", e))?;
    
    // Set up worker pool
    let (tx, rx) = mpsc::channel(100); // Buffer size of 100
    let num_workers = num_cpus::get().min(8); // Use up to 8 worker threads
    let mut workers = JoinSet::new();
    
    // Start workers
    let server_root = Arc::new(server_root.to_path_buf());
    for _ in 0..num_workers {
        let worker_rx = rx.clone();
        let worker_regex = Arc::clone(&regex);
        let worker_root = Arc::clone(&server_root);
        workers.spawn(search_worker(
            worker_rx,
            worker_regex,
            params.context_lines,
            worker_root,
        ));
    }
    drop(rx); // Drop the original receiver
    
    // Setup timeout
    let timeout = Duration::from_secs(params.timeout_secs);
    let start_time = Instant::now();
    
    // Start directory traversal
    let walker = WalkDir::new(root_path)
        .follow_links(false)
        .max_depth(if params.recursive { usize::MAX } else { 1 })
        .into_iter();
    
    let mut files_searched = 0;
    
    // Process files
    for entry in walker.filter_map(Result::ok) {
        // Check if we've exceeded the timeout
        if start_time.elapsed() > timeout {
            warn!("Search operation timed out after {} seconds", params.timeout_secs);
            break;
        }
        
        match should_process_entry(&entry, &file_pattern, params.max_file_size) {
            Ok(true) => {
                files_searched += 1;
                if tx.send(entry.path().to_path_buf()).await.is_err() {
                    // All receivers have been dropped
                    break;
                }
            }
            Ok(false) => {} // Skip this file
            Err(e) => {
                warn!("Error processing entry {}: {}", entry.path().display(), e);
            }
        }
    }
    
    // Drop sender to signal workers to finish
    drop(tx);
    
    // Collect results from workers
    let mut all_matches = Vec::new();
    let mut total_matches = 0;
    
    while let Some(result) = workers.join_next().await {
        match result {
            Ok(file_matches) => {
                for file_match in file_matches {
                    total_matches += file_match.matches.len();
                    all_matches.push(file_match);
                    
                    // Check if we've reached the maximum number of results
                    if total_matches >= params.max_results {
                        break;
                    }
                }
            }
            Err(e) => {
                error!("Worker task failed: {}", e);
            }
        }
        
        // Check if we've reached the maximum number of results
        if total_matches >= params.max_results {
            break;
        }
    }
    
    // Cancel any remaining workers
    workers.abort_all();
    
    // Sort results by file path for consistent output
    all_matches.sort_by(|a, b| a.file.cmp(&b.file));
    
    Ok(SearchResults {
        total_matches,
        files_searched,
        files_matched: all_matches.len(),
        matches: all_matches,
    })
}

// Define the schema for the tool
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root_path": {
                "type": "string",
                "description": "Root directory to start the search from (relative to server root)"
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
pub async fn execute(args: &Value, server_root: &Path) -> Result<ToolCallResult> {
    // Parse arguments
    let params: SearchParams = match serde_json::from_value(args.clone()) {
        Ok(params) => params,
        Err(e) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Invalid parameters: {}", e),
                }],
                is_error: Some(true),
            });
        }
    };

    debug!(
        "Searching for '{}' in '{}', recursive: {}, case_sensitive: {}, regex: {}",
        params.pattern, params.root_path, params.recursive, params.case_sensitive, params.regex
    );

    // Perform the search
    match search_files(&params, server_root).await {
        Ok(results) => {
            // Format results
            if results.total_matches == 0 {
                return Ok(ToolCallResult {
                    content: vec![ToolContent::Text {
                        text: format!(
                            "No matches found for pattern '{}' in '{}'.\nSearched {} files.",
                            params.pattern, params.root_path, results.files_searched
                        ),
                    }],
                    is_error: Some(false),
                });
            }

            // Format as JSON or text based on result complexity
            if results.total_matches > 10 || results.files_matched > 3 {
                // Return JSON for complex results
                return Ok(ToolCallResult {
                    content: vec![ToolContent::Json {
                        json: serde_json::to_value(results)?,
                    }],
                    is_error: Some(false),
                });
            } else {
                // Format simple results as text
                let mut text = format!(
                    "Found {} matches in {} files (searched {} total):\n\n",
                    results.total_matches, results.files_matched, results.files_searched
                );

                for file_match in results.matches {
                    writeln!(&mut text, "File: {}", file_match.file)?;
                    
                    for m in file_match.matches {
                        writeln!(&mut text, "  Line {}: {}", m.line_number, m.line.trim())?;
                        
                        if !m.context.is_empty() {
                            for ctx in m.context {
                                writeln!(
                                    &mut text,
                                    "    {} | {}", 
                                    ctx.line_number,
                                    ctx.content.trim()
                                )?;
                            }
                            writeln!(&mut text)?;
                        }
                    }
                    writeln!(&mut text)?;
                }

                return Ok(ToolCallResult {
                    content: vec![ToolContent::Text { text }],
                    is_error: Some(false),
                });
            }
        }
        Err(e) => {
            return Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: format!("Search error: {}", e),
                }],
                is_error: Some(true),
            });
        }
    }
}
