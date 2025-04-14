use anyhow::Result;
use clap::Parser;
use mcp_protocol::types::tool::{ToolCallResult, ToolContent};
use mcp_server::{ServerBuilder, transport::StdioTransport};
use serde_json::json;
use std::{
    env,
    path::{Path, PathBuf},
};
use tracing::{debug, info, warn, Level};
use tracing_subscriber::{fmt, EnvFilter};

mod tools;
mod utils;

#[derive(Parser, Debug)]
#[clap(
    name = "fs-mcp-server",
    about = "MCP server providing secure filesystem access",
    version
)]
struct CliArgs {
    /// Root directory for filesystem operations
    #[clap(long, env = "FS_SERVER_ROOT")]
    root_dir: Option<PathBuf>,

    /// Maximum file size for read operations (in bytes)
    #[clap(long, env = "FS_MAX_FILE_SIZE", default_value = "10485760")]
    max_file_size: u64,

    /// Request timeout in seconds
    #[clap(long, env = "FS_REQUEST_TIMEOUT", default_value = "30")]
    request_timeout: u64,

    /// Log level
    #[clap(long, env = "FS_LOG_LEVEL", default_value = "info")]
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

    // Determine root directory
    let root_dir = args.root_dir
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Canonicalize root path
    let root_dir = match root_dir.canonicalize() {
        Ok(path) => path,
        Err(e) => {
            panic!("Failed to canonicalize root directory: {}", e);
        }
    };

    info!("Starting fs-mcp-server with root directory: {}", root_dir.display());
    info!("Max file size: {} bytes", args.max_file_size);
    info!("Request timeout: {} seconds", args.request_timeout);

    // Create and build server
    let server = build_server(root_dir, args.max_file_size, args.request_timeout)?;

    // Run server
    info!("Server initialized. Waiting for client connection...");
    server.run().await?;

    info!("Server shutting down");
    Ok(())
}

// Setup logging with optional file output
fn setup_logging(log_level: &str, log_file: Option<&Path>) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            EnvFilter::new(format!("fs_mcp_server={}", log_level))
        });

    let subscriber = fmt::Subscriber::builder()
        .with_env_filter(env_filter)
        .with_ansi(atty::is(atty::Stream::Stdout));

    if let Some(log_file) = log_file {
        // Create parent directories if they don't exist
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)?;

        let subscriber = subscriber
            .with_writer(move || -> Box<dyn std::io::Write> {
                Box::new(std::io::BufWriter::new(
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(log_file)
                        .unwrap(),
                ))
            })
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set global default subscriber");
    } else {
        // Log to stderr
        let subscriber = subscriber.finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set global default subscriber");
    }

    Ok(())
}

// Build the MCP server with all filesystem tools
fn build_server(
    root_dir: PathBuf,
    max_file_size: u64,
    request_timeout: u64,
) -> Result<mcp_server::Server> {
    // Create a new server builder
    let mut server_builder = ServerBuilder::new("filesystem-server", "0.1.0")
        .with_transport(StdioTransport::new());
    
    // Add the list tool
    server_builder = server_builder.with_tool(
        "fs.list",
        Some("List files in a directory"),
        tools::list::schema(),
        {
            let root = root_dir.clone();
            move |args| tools::list::execute(args, &root)
        }
    );
    
    // Add the read tool
    server_builder = server_builder.with_tool(
        "fs.read",
        Some("Read file contents"),
        tools::read::schema(),
        {
            let root = root_dir.clone();
            let max_size = max_file_size;
            move |args| tools::read::execute(args, &root, max_size)
        }
    );
    
    // Add the search tool
    server_builder = server_builder.with_tool(
        "fs.search",
        Some("Search file contents for matching patterns"),
        tools::search::schema(),
        {
            let root = root_dir.clone();
            move |args| {
                Box::pin(async move {
                    tools::search::execute(args, &root).await
                })
            }
        }
    );
    
    // Build and return the server
    server_builder.build()
}
