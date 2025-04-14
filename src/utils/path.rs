use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Error, Debug)]
pub enum PathError {
    #[error("Path is outside of all allowed directories")]
    OutsideAllowedPaths,
    
    #[error("Path not found")]
    NotFound,
    
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
}

/// Manages a set of allowed directories for filesystem operations
#[derive(Clone)]
pub struct AllowedPaths {
    paths: Vec<PathBuf>,
}

impl AllowedPaths {
    /// Create a new AllowedPaths from a list of directory paths
    ///
    /// # Arguments
    ///
    /// * `paths` - List of directory paths to allow
    ///
    /// # Returns
    ///
    /// * `Result<Self, PathError>` - A new AllowedPaths instance or an error
    pub fn new(paths: Vec<PathBuf>) -> Result<Self, PathError> {
        if paths.is_empty() {
            return Err(PathError::IoError(io::Error::new(
                io::ErrorKind::InvalidInput,
                "No allowed directories specified"
            )));
        }
        
        // Canonicalize all paths
        let mut canonicalized_paths = Vec::new();
        for path in paths {
            match path.canonicalize() {
                Ok(canonical) => canonicalized_paths.push(canonical),
                Err(e) => {
                    warn!("Failed to canonicalize allowed path: {}", path.display());
                    return Err(PathError::IoError(e));
                },
            }
        }
        
        debug!("Initialized allowed paths: {:?}", canonicalized_paths);
        
        Ok(AllowedPaths { paths: canonicalized_paths })
    }
    
    /// Validate a path to ensure it's within any of the allowed directories
    ///
    /// # Arguments
    ///
    /// * `path` - The path to validate (absolute path)
    ///
    /// # Returns
    ///
    /// * `Ok(PathBuf)` - The canonicalized path if valid
    /// * `Err(PathError)` - If the path is invalid or outside all allowed directories
    pub fn validate_path(&self, path: &Path) -> Result<PathBuf, PathError> {
        debug!("Validating path: '{}'", path.display());
        
        // Try to canonicalize the path
        let canonical_path = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    // Special case for creation operations where the path doesn't exist yet
                    // In this case, we validate the parent directory if it exists
                    if let Some(parent) = path.parent() {
                        if parent.exists() {
                            let parent_canonical = parent.canonicalize()?;
                            
                            // Check if the parent path is within any allowed directory
                            if !self.is_path_allowed(&parent_canonical) {
                                warn!("Parent path is outside all allowed directories: '{}'", parent.display());
                                return Err(PathError::OutsideAllowedPaths);
                            }
                            
                            // For non-existent files, return the original path
                            return Ok(path.to_path_buf());
                        }
                    }
                    
                    debug!("Path not found: '{}'", path.display());
                    return Err(PathError::NotFound);
                } else {
                    warn!("Failed to canonicalize path: {}", e);
                    return Err(PathError::IoError(e));
                }
            }
        };
        
        // Check if the path is within any allowed directory
        if !self.is_path_allowed(&canonical_path) {
            warn!(
                "Path '{}' resolves to '{}' which is outside all allowed directories",
                path.display(),
                canonical_path.display()
            );
            return Err(PathError::OutsideAllowedPaths);
        }
        
        debug!("Path '{}' validated successfully", path.display());
        Ok(canonical_path)
    }
    
    /// Check if a canonicalized path is within any of the allowed directories
    ///
    /// # Arguments
    ///
    /// * `path` - The canonicalized path to check
    ///
    /// # Returns
    ///
    /// * `bool` - True if the path is allowed, false otherwise
    fn is_path_allowed(&self, path: &Path) -> bool {
        for allowed_path in &self.paths {
            if path.starts_with(allowed_path) {
                return true;
            }
        }
        false
    }
    
    /// Get the closest relative path from any of the allowed directories
    ///
    /// # Arguments
    ///
    /// * `path` - The absolute path to convert
    ///
    /// # Returns
    ///
    /// * `String` - The path with the shortest relative representation,
    ///              or the original path if it cannot be made relative
    pub fn closest_relative_path(&self, path: &Path) -> String {
        let mut best_relative = path.to_string_lossy().into_owned();
        let mut best_components = usize::MAX;
        
        for allowed_path in &self.paths {
            if let Ok(rel_path) = path.strip_prefix(allowed_path) {
                let component_count = rel_path.components().count();
                if component_count < best_components {
                    best_relative = rel_path.to_string_lossy().into_owned();
                    best_components = component_count;
                }
            }
        }
        
        best_relative
    }
    
    /// Get a list of all allowed directories
    ///
    /// # Returns
    ///
    /// * `Vec<PathBuf>` - List of all allowed directories (canonicalized)
    pub fn all_paths(&self) -> &Vec<PathBuf> {
        &self.paths
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
    fn test_allowed_paths_single_directory() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path().to_path_buf();
        
        // Create a file within the root
        let test_file = root.join("test.txt");
        fs::write(&test_file, "test content").unwrap();
        
        // Create AllowedPaths with a single directory
        let allowed_paths = AllowedPaths::new(vec![root.clone()]).unwrap();
        
        // Validate a path within the allowed directory
        let result = allowed_paths.validate_path(&test_file);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_file);
    }
    
    #[test]
    fn test_allowed_paths_multiple_directories() {
        let temp_dir1 = tempdir().unwrap();
        let temp_dir2 = tempdir().unwrap();
        
        // Create a file in each directory
        let test_file1 = temp_dir1.path().join("test1.txt");
        let test_file2 = temp_dir2.path().join("test2.txt");
        fs::write(&test_file1, "test content 1").unwrap();
        fs::write(&test_file2, "test content 2").unwrap();
        
        // Create AllowedPaths with multiple directories
        let allowed_paths = AllowedPaths::new(vec![
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
        ]).unwrap();
        
        // Validate paths in both directories
        let result1 = allowed_paths.validate_path(&test_file1);
        let result2 = allowed_paths.validate_path(&test_file2);
        
        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(result1.unwrap(), test_file1);
        assert_eq!(result2.unwrap(), test_file2);
    }
    
    #[test]
    fn test_validate_path_outside_allowed_paths() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path().to_path_buf();
        
        // Create AllowedPaths with a single directory
        let allowed_paths = AllowedPaths::new(vec![root.clone()]).unwrap();
        
        // Create a file outside the allowed directory
        let outside_dir = tempdir().unwrap();
        let outside_file = outside_dir.path().join("outside.txt");
        fs::write(&outside_file, "outside content").unwrap();
        
        // Validate a path outside the allowed directory
        let result = allowed_paths.validate_path(&outside_file);
        
        match result {
            Err(PathError::OutsideAllowedPaths) => {}
            _ => panic!("Expected OutsideAllowedPaths error"),
        }
    }
    
    #[test]
    fn test_closest_relative_path() {
        let temp_dir1 = tempdir().unwrap();
        let temp_dir2 = tempdir().unwrap();
        
        // Create nested structure in the second directory
        let nested_dir = temp_dir2.path().join("nested");
        fs::create_dir_all(&nested_dir).unwrap();
        
        let file_in_nested = nested_dir.join("file.txt");
        fs::write(&file_in_nested, "nested content").unwrap();
        
        // Create AllowedPaths with both directories
        let allowed_paths = AllowedPaths::new(vec![
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
            nested_dir.clone(),
        ]).unwrap();
        
        // Test closest relative path selection
        // From nested dir (should be relative to nested_dir not temp_dir2)
        let relative = allowed_paths.closest_relative_path(&file_in_nested);
        assert_eq!(relative, "file.txt");
    }
}
