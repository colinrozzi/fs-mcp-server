#!/bin/bash

# Build the server in debug mode
echo "Building fs-mcp-server..."
cargo build

# Check if build succeeded
if [ $? -ne 0 ]; then
    echo "Build failed!"
    exit 1
fi

echo "Build successful."

# Run with multiple allowed directories
echo "Running server with multiple allowed directories..."

# Default to current directory and parent directory if no args provided
if [ "$#" -eq 0 ]; then
    ALLOWED_DIRS="$(pwd),$(dirname "$(pwd)")"
    echo "Using default allowed directories: $ALLOWED_DIRS"
    ./target/debug/fs-mcp-server --allowed-dirs "$ALLOWED_DIRS"
else
    # Pass all arguments to the server
    echo "Using command line arguments: $@"
    ./target/debug/fs-mcp-server "$@"
fi
