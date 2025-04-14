use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Error, Debug)]
pub enum PathError {
    #[error("Path is outside of the allowed root directory")]
    OutsideRoot,
    
    #[error("Path not found")]
    NotFound,
    
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
}

/// Validate a path to ensure it's within the allowed server root directory
///
/// This function:
/// 1. Resolves path to an absolute path
/// 2. Checks if the path is within the root directory
/// 3. Returns the canonicalized path if valid
///
/// # Arguments
///
/// * `path` - The path to validate (relative to the server root)
/// * `root` - The server root directory (absolute path)
///
/// # Returns
///
/// * `Ok(PathBuf)` - The canonicalized path if valid
/// * `Err(PathError)` - If the path is invalid or outside the root
pub fn validate_path(path: &str, root: &Path) -> Result<PathBuf, PathError> {
    debug!("Validating path: '{}' against root: '{}'", path, root.display());
    
    // Ensure root is absolute and canonical
    let root = match root.canonicalize() {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to canonicalize root directory: {}", e);
            return Err(PathError::IoError(e));
        }
    };
    
    // Join path with root to get the absolute path
    let full_path = root.join(path);
    
    // Try to canonicalize the path
    let canonical_path = match full_path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                // Special case for creation operations where the path doesn't exist yet
                // In this case, we validate the parent directory if it exists
                if let Some(parent) = full_path.parent() {
                    if parent.exists() {
                        let parent_canonical = parent.canonicalize()?;
                        if !parent_canonical.starts_with(&root) {
                            warn!("Parent path is outside root: '{}'", parent.display());
                            return Err(PathError::OutsideRoot);
                        }
                        
                        // For non-existent files, return the original joined path
                        // (not canonicalized since it doesn't exist)
                        return Ok(full_path);
                    }
                }
                
                debug!("Path not found: '{}'", full_path.display());
                return Err(PathError::NotFound);
            } else {
                warn!("Failed to canonicalize path: {}", e);
                return Err(PathError::IoError(e));
            }
        }
    };
    
    // Check if the canonicalized path is within the root
    if !canonical_path.starts_with(&root) {
        warn!(
            "Path '{}' resolves to '{}' which is outside root '{}'",
            path,
            canonical_path.display(),
            root.display()
        );
        return Err(PathError::OutsideRoot);
    }
    
    debug!("Path '{}' validated successfully", path);
    Ok(canonical_path)
}

/// Get a path relative to the server root
///
/// # Arguments
///
/// * `path` - The absolute path to convert
/// * `root` - The server root directory
///
/// # Returns
///
/// * `String` - The path relative to the root, or the original path 
///             if it cannot be made relative
pub fn relative_to_root(path: &Path, root: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel_path) => rel_path.to_string_lossy().into_owned(),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Check if a path is a possible binary file based on extension
///
/// # Arguments
///
/// * `path` - The path to check
///
/// # Returns
///
/// * `bool` - True if the file might be binary
pub fn is_likely_binary_by_extension(path: &Path) -> bool {
    const BINARY_EXTENSIONS: &[&str] = &[
        "exe", "dll", "so", "dylib", "bin", "obj", "o", "a", "lib",
        "png", "jpg", "jpeg", "gif", "bmp", "tiff", "ico",
        "mp3", "mp4", "avi", "mov", "wmv", "flv", "wav",
        "zip", "tar", "gz", "bz2", "xz", "7z", "rar",
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    ];
    
    if let Some(ext) = path.extension() {
        if let Some(ext_str) = ext.to_str() {
            return BINARY_EXTENSIONS.contains(&ext_str.to_lowercase().as_str());
        }
    }
    
    false
}

/// Check if a file is a text file by examining its content
///
/// # Arguments
///
/// * `path` - The path to the file
///
/// # Returns
///
/// * `Result<bool, io::Error>` - True if the file is likely text, error if file can't be read
pub fn is_text_file(path: &Path) -> Result<bool, io::Error> {
    // Quick check based on extension
    if is_likely_binary_by_extension(path) {
        return Ok(false);
    }
    
    // Open the file
    let mut file = std::fs::File::open(path)?;
    
    // Read a sample of the file to check for binary content
    let mut buffer = [0u8; 8192]; // Read up to 8KB
    let bytes_read = std::io::Read::read(&mut file, &mut buffer)?;
    
    // If we couldn't read anything, assume it's not a text file
    if bytes_read == 0 {
        return Ok(true); // Empty files are considered text
    }
    
    // Check for null bytes or high density of non-ASCII characters
    let mut null_bytes = 0;
    let mut non_ascii = 0;
    
    for &byte in &buffer[0..bytes_read] {
        if byte == 0 {
            null_bytes += 1;
        } else if byte > 127 {
            non_ascii += 1;
        }
    }
    
    // Heuristics to determine if file is binary:
    // 1. More than 1% null bytes is likely binary
    // 2. More than 30% non-ASCII could be binary unless it's UTF-8
    
    if null_bytes > bytes_read / 100 {
        return Ok(false);
    }
    
    if non_ascii > bytes_read * 3 / 10 {
        // Additional UTF-8 validation for high non-ASCII content
        let is_valid_utf8 = String::from_utf8(buffer[0..bytes_read].to_vec()).is_ok();
        return Ok(is_valid_utf8);
    }
    
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
    
    #[test]
    fn test_validate_path_within_root() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        
        // Create a file within the root
        let test_file = root.join("test.txt");
        fs::write(&test_file, "test content").unwrap();
        
        // Validate a path within root
        let result = validate_path("test.txt", root);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_file);
    }
    
    #[test]
    fn test_validate_path_outside_root() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        
        // Attempt path traversal
        let result = validate_path("../outside.txt", root);
        assert!(result.is_err());
        match result {
            Err(PathError::NotFound) => {}
            _ => panic!("Expected NotFound error"),
        }
    }
    
    #[test]
    fn test_validate_non_existent_path() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        
        // Try to validate a path that doesn't exist
        let result = validate_path("nonexistent.txt", root);
        
        // For non-existent files, we should get a NotFound error
        match result {
            Err(PathError::NotFound) => {}
            _ => panic!("Expected NotFound error"),
        }
    }
    
    #[test]
    fn test_relative_to_root() {
        let root = Path::new("/tmp/root");
        let path = Path::new("/tmp/root/subdir/file.txt");
        
        assert_eq!(relative_to_root(path, root), "subdir/file.txt");
    }
    
    #[test]
    fn test_is_likely_binary_by_extension() {
        assert!(is_likely_binary_by_extension(Path::new("test.exe")));
        assert!(is_likely_binary_by_extension(Path::new("image.png")));
        assert!(!is_likely_binary_by_extension(Path::new("file.txt")));
        assert!(!is_likely_binary_by_extension(Path::new("script.py")));
    }
}
