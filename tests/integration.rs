use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_basic_functionality() {
    let temp_dir = tempdir().unwrap();
    let test_file_path = temp_dir.path().join("test.txt");
    
    // Create a test file
    std::fs::write(&test_file_path, "This is a test file\nWith searchable content").unwrap();
    
    // Get path to the built executable
    let executable_path = std::env::current_dir().unwrap()
        .join("target/debug/fs-mcp-server");
    
    assert!(executable_path.exists(), "Executable not found at {:?}", executable_path);
    
    // Run the executable with --help to verify it works
    let output = Command::new(executable_path)
        .arg("--help")
        .output()
        .expect("Failed to execute command");
    
    // Check it ran successfully
    assert!(output.status.success());
    
    // Check the output contains the expected help text
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MCP server providing secure filesystem access"));
    assert!(stdout.contains("--root-dir"));
    assert!(stdout.contains("--max-file-size"));
    
    println!("Basic functionality test passed!");
}
