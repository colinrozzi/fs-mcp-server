use anyhow::{Context, Result};
use clap::Parser;
use modelcontextprotocol_server::{transport::StdioTransport, ServerBuilder};
use std::{
    env, fs,
    io::{self, BufRead},
    path::{Path, PathBuf},
};
use tracing::{info, Level};
use tracing_subscriber::{self, fmt, EnvFilter};

mod tools;
mod utils;

use utils::path::AllowedPaths;

#[derive(Parser, Debug)]
#[clap(
    name = "fs-mcp-server",
    about = "MCP server providing secure filesystem access",
    version
)]
struct CliArgs {
    /// Allowed directories for filesystem operations (comma-separated)
    #[clap(long, env = "FS_ALLOWED_DIRS", value_delimiter = ',')]
    allowed_dirs: Option<Vec<PathBuf>>,

    /// Path to a configuration file listing allowed directories (one per line)
    #[clap(long, env = "FS_CONFIG_FILE")]
    config_file: Option<PathBuf>,

    /// Maximum file size for read operations (in bytes)
    #[clap(long, env = "FS_MAX_FILE_SIZE", default_value = "10485760")]
    max_file_size: u64,

    /// Request timeout in seconds
    #[clap(long, env = "FS_REQUEST_TIMEOUT", default_value = "30")]
    request_timeout: u64,

    /// Log level
    #[clap(long, env = "FS_LOG_LEVEL", default_value = "debug")]
    log_level: String,

    /// Log file path
    #[clap(long, env = "FS_LOG_FILE")]
    log_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = CliArgs::parse();

    // Setup logging
    setup_logging(&args.log_level, args.log_file.as_deref())?;

    // Determine allowed directories
    let allowed_dirs =
        determine_allowed_dirs(&args).context("Failed to determine allowed directories")?;

    // Initialize the AllowedPaths struct
    let allowed_paths =
        AllowedPaths::new(allowed_dirs).context("Failed to initialize allowed paths")?;

    info!("Starting fs-mcp-server");
    info!("Allowed directories:");
    for (i, path) in allowed_paths.all_paths().iter().enumerate() {
        info!("  {}: {}", i + 1, path.display());
    }
    info!("Max file size: {} bytes", args.max_file_size);
    info!("Request timeout: {} seconds", args.request_timeout);

    // Create and build server
    let server = build_server(allowed_paths, args.max_file_size)?;

    // Run server
    info!("Server initialized. Waiting for client connection...");
    server.run().await?;

    info!("Server shutting down");
    Ok(())
}

/// Determine the list of allowed directories from command-line args and config file
fn determine_allowed_dirs(args: &CliArgs) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();

    // Process command-line allowed_dirs
    if let Some(arg_dirs) = &args.allowed_dirs {
        dirs.extend(arg_dirs.clone());
    }

    // Process config file if specified
    if let Some(config_path) = &args.config_file {
        let dirs_from_config = read_allowed_dirs_from_config(config_path).context(format!(
            "Failed to read config file: {}",
            config_path.display()
        ))?;
        dirs.extend(dirs_from_config);
    }

    // If no directories specified, use current directory
    if dirs.is_empty() {
        dirs.push(env::current_dir()?);
    }

    Ok(dirs)
}

/// Read allowed directories from a configuration file (one directory per line)
fn read_allowed_dirs_from_config(config_path: &Path) -> Result<Vec<PathBuf>> {
    let file = fs::File::open(config_path)?;
    let reader = io::BufReader::new(file);
    let mut dirs = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        // Skip empty lines and comments
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            dirs.push(PathBuf::from(trimmed));
        }
    }

    Ok(dirs)
}

// Setup logging with optional file output
fn setup_logging(log_level: &str, log_file: Option<&Path>) -> Result<()> {
    // Parse the log level
    let _level = match log_level.to_lowercase().as_str() {
        "error" => Level::ERROR,
        "warn" => Level::WARN,
        "info" => Level::INFO,
        "debug" => Level::DEBUG,
        "trace" => Level::TRACE,
        _ => Level::INFO,
    };

    let log_file = if let Some(path) = log_file {
        Some(path.to_path_buf())
    } else {
        Some("/Users/colinrozzi/work/mcp-servers/fs-mcp-server/logs/fs-mcp-server.log".into())
    };

    // Clear existing log file if it exists
    if let Some(log_path) = log_file.as_ref() {
        std::fs::remove_file(log_path).ok();
    }

    // Create the logger
    if let Some(log_file_path) = log_file {
        // Create parent directories if they don't exist
        if let Some(parent) = log_file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Get a static string path to use in the closure
        let log_path = log_file_path.to_path_buf();

        // Create a file subscriber with an env_filter to capture logs from all relevant crates

        // This filter will include all mcp-related crates at the specified level
        // We also include this crate's logs
        //     let filter_level = log_level.to_lowercase();
        let filter_level = "debug";
        let filter = EnvFilter::new(format!(
            "mcp_server={0},mcp_protocol={0},mcp_client={0},fs_mcp_server={0}",
            filter_level
        ));

        let file_subscriber = fmt::Subscriber::builder()
            .with_env_filter(filter)
            .with_writer(move || -> Box<dyn std::io::Write> {
                let path = log_path.clone();
                Box::new(std::io::BufWriter::new(
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .unwrap(),
                ))
            })
            .with_ansi(false)
            .finish();

        // Set up the subscriber
        tracing::subscriber::set_global_default(file_subscriber)
            .expect("Failed to set global default subscriber");

        info!(
            "Logging initialized at '{}' level, capturing logs from mcp_* crates",
            log_level
        );
    }

    Ok(())
}

// Build the MCP server with all filesystem tools
fn build_server(allowed_paths: AllowedPaths, max_file_size: u64) -> Result<modelcontextprotocol_server::Server> {
    // Create a new server builder
    let mut server_builder =
        ServerBuilder::new("filesystem-server", "0.1.0").with_transport(StdioTransport::new());

    // Add the list tool
    server_builder = server_builder.with_tool(
        "list",
        Some("List files in a directory"),
        tools::list::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::list::execute(&args, &paths)
        },
    );

    // Add the read tool
    server_builder =
        server_builder.with_tool("read", Some("Read file contents"), tools::read::schema(), {
            let paths = allowed_paths.clone();
            let max_size = max_file_size;
            move |args| tools::read::execute(&args, &paths, max_size)
        });

    // Add the write tool
    server_builder = server_builder.with_tool(
        "write",
        Some("Write content to a file"),
        tools::write::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::write::execute(&args, &paths)
        },
    );

    // Add the mkdir tool
    server_builder = server_builder.with_tool(
        "mkdir",
        Some("Create directories"),
        tools::mkdir::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::mkdir::execute(&args, &paths)
        },
    );

    // Add the delete tool
    server_builder = server_builder.with_tool(
        "delete",
        Some("Delete files or directories"),
        tools::delete::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::delete::execute(&args, &paths)
        },
    );

    // Add the copy tool
    server_builder = server_builder.with_tool(
        "copy",
        Some("Copy files or directories"),
        tools::copy::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::copy::execute(&args, &paths)
        },
    );

    // Add the move tool
    server_builder = server_builder.with_tool(
        "move",
        Some("Move or rename files or directories"),
        tools::move_file::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::move_file::execute(&args, &paths)
        },
    );

    // Add the info tool
    server_builder = server_builder.with_tool(
        "info",
        Some("Get detailed information about a file or directory"),
        tools::info::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::info::execute(&args, &paths)
        },
    );

    // Add the search tool
    server_builder = server_builder.with_tool(
        "search",
        Some("Search file contents for matching patterns"),
        tools::search::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::search::execute(&args, &paths)
        },
    );

    // Add the list_allowed_dirs tool
    server_builder = server_builder.with_tool(
        "list_allowed_dirs",
        Some("List all allowed directories"),
        tools::list_allowed_dirs::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::list_allowed_dirs::execute(&args, &paths)
        },
    );

    // Add the edit tool
    server_builder = server_builder.with_tool(
        "edit",
        Some("Perform partial edits on a file"),
        tools::edit::schema(),
        {
            let paths = allowed_paths.clone();
            move |args| tools::edit::execute(&args, &paths)
        },
    );

    // Build and return the server
    server_builder.build()
}
