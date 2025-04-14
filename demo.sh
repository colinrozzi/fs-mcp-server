#!/bin/bash

# Build the server in debug mode
echo "Building fs-mcp-server and simple client example..."
cargo build --example simple_client

# Check if build succeeded
if [ $? -ne 0 ]; then
    echo "Build failed!"
    exit 1
fi

echo "Build successful."

# Run the simple client example to demonstrate server functionality
echo "Running simple client example to demonstrate multi-directory support..."
cargo run --example simple_client

# Note: The simple_client example is now configured to use both the current 
# directory and the parent directory as allowed directories
