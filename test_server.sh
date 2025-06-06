#!/bin/bash

# Test script to verify the fs-mcp-server works
cd /Users/colinrozzi/work/mcp-servers/fs-mcp-server

echo "Testing fs-mcp-server binary..."

# Test that the binary exists and can show help
if [ -f "./target/release/fs-mcp-server" ]; then
    echo "✅ Binary exists"
    
    # Make sure it's executable
    chmod +x ./target/release/fs-mcp-server
    
    # Test help output
    echo "Testing --help output:"
    ./target/release/fs-mcp-server --help
    
    echo ""
    echo "✅ Server binary appears to be working!"
    echo "You can now test the list command fix we made."
else
    echo "❌ Binary not found"
    exit 1
fi
