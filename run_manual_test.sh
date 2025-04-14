#!/bin/bash
set -e

# Build the server
echo "Building the server..."
cargo build

# Compile and run the manual test
echo "Compiling and running manual test..."
rustc -o manual_test manual_test.rs --extern mcp_client="$(find target/debug/deps -name "libmcp_client-*.rlib" | head -n 1)" \
    --extern anyhow="$(find target/debug/deps -name "libanyhow-*.rlib" | head -n 1)" \
    --extern serde_json="$(find target/debug/deps -name "libserde_json-*.rlib" | head -n 1)" \
    --extern tokio="$(find target/debug/deps -name "libtokio-*.rlib" | head -n 1)" \
    --edition 2021

echo "Running the test..."
./manual_test
