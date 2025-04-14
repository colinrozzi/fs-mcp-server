#!/bin/bash
set -e

# Build the server and example
echo "Building the server and example..."
cargo build
cargo build --example simple_client

# Run the example
echo "Running the simple client example..."
cargo run --example simple_client
