#!/bin/bash
set -e

# Build the server and example
echo "Building the server and example..."
cargo build
cargo build --example enhanced_client

# Run the enhanced client example
echo "Running the enhanced client example..."
cargo run --example enhanced_client
