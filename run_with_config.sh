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

# Check if the configuration file exists
CONFIG_FILE="allowed_dirs.conf"
if [ ! -f "$CONFIG_FILE" ]; then
    echo "Configuration file $CONFIG_FILE not found!"
    echo "Creating a sample configuration file..."
    
    cat > "$CONFIG_FILE" << EOF
# Configuration file for fs-mcp-server allowed directories
# Each line specifies one allowed directory path

# Current directory
.

# Parent directory
..

# You can add absolute paths as well
# /home/user/documents
# /var/data/shared

# Lines starting with # are comments and will be ignored
# Empty lines are also ignored
EOF

    echo "Created sample configuration file: $CONFIG_FILE"
fi

# Run the server with the configuration file
echo "Running server with configuration file: $CONFIG_FILE"
./target/debug/fs-mcp-server --config-file "$CONFIG_FILE" --log-level debug
