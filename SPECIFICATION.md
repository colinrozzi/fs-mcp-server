# Filesystem MCP Server Specification

## Overview

The Filesystem MCP Server is a Model Context Protocol (MCP) implementation that provides secure access to the local filesystem. It enables clients to perform common file operations through a standardized interface while maintaining proper security boundaries.

## Server Information

- **Name:** `filesystem-server`
- **Version:** `0.1.0`
- **Description:** A secure MCP server for filesystem operations

## Security Model

### Path Validation

All operations are constrained to a configurable root directory. The server will:

1. Canonicalize all paths to resolve symlinks and relative paths
2. Validate that the resulting path is contained within the designated root directory
3. Reject any attempts to access paths outside the root with appropriate error messages

### Configuration

The root directory can be specified through:
- Environment variable: `FS_SERVER_ROOT`
- Command-line argument: `--root-dir <path>`
- Default: Current working directory if neither is specified

## Tools

The server provides the following tools:

### 1. `list`

Lists files and directories at a specified path.

**Parameters:**
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to list files from (relative to server root)"
    },
    "pattern": {
      "type": "string",
      "description": "Optional glob pattern to filter files",
      "default": "*"
    },
    "recursive": {
      "type": "boolean",
      "description": "Whether to list files recursively",
      "default": false
    },
    "include_hidden": {
      "type": "boolean",
      "description": "Whether to include hidden files (starting with .)",
      "default": false
    },
    "metadata": {
      "type": "boolean",
      "description": "Whether to include file metadata (size, type, modification time)",
      "default": true
    }
  },
  "required": ["path"]
}
```

**Response:**
```json
{
  "entries": [
    {
      "name": "example.txt",
      "path": "path/to/example.txt",
      "type": "file",
      "size": 1024,
      "modified": "2024-04-10T15:30:00Z",
      "is_hidden": false
    },
    {
      "name": "documents",
      "path": "path/to/documents",
      "type": "directory",
      "modified": "2024-04-09T12:00:00Z",
      "is_hidden": false
    }
  ]
}
```

### 2. `read`

Reads file contents with support for different encodings and partial reads.

**Parameters:**
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to the file to read (relative to server root)"
    },
    "encoding": {
      "type": "string",
      "description": "File encoding",
      "enum": ["utf8", "base64", "binary"],
      "default": "utf8"
    },
    "start_line": {
      "type": "integer",
      "description": "Start line for partial read (0-indexed)"
    },
    "end_line": {
      "type": "integer",
      "description": "End line for partial read (inclusive)"
    },
    "max_size": {
      "type": "integer",
      "description": "Maximum number of bytes to read",
      "default": 1048576
    }
  },
  "required": ["path"]
}
```

**Response:**
```json
{
  "content": "File content as string or base64-encoded data",
  "encoding": "utf8",
  "size": 1024,
  "truncated": false,
  "line_count": 20,
  "metadata": {
    "path": "path/to/file.txt",
    "modified": "2024-04-10T15:30:00Z",
    "size": 1024
  }
}
```

### 3. `write`

Creates or updates files with specified content.

**Parameters:**
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to the file to write (relative to server root)"
    },
    "content": {
      "type": "string",
      "description": "Content to write to the file"
    },
    "encoding": {
      "type": "string",
      "description": "Content encoding",
      "enum": ["utf8", "base64"],
      "default": "utf8"
    },
    "mode": {
      "type": "string",
      "description": "Write mode",
      "enum": ["create", "overwrite", "append", "create_new"],
      "default": "overwrite"
    },
    "make_dirs": {
      "type": "boolean",
      "description": "Create parent directories if they don't exist",
      "default": false
    }
  },
  "required": ["path", "content"]
}
```

**Response:**
```json
{
  "success": true,
  "path": "path/to/file.txt",
  "bytes_written": 1024,
  "metadata": {
    "path": "path/to/file.txt",
    "modified": "2024-04-10T15:30:00Z",
    "size": 1024
  }
}
```

### 4. `mkdir`

Creates directories.

**Parameters:**
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to the directory to create (relative to server root)"
    },
    "recursive": {
      "type": "boolean",
      "description": "Create parent directories if they don't exist",
      "default": true
    }
  },
  "required": ["path"]
}
```

**Response:**
```json
{
  "success": true,
  "path": "path/to/new_directory"
}
```

### 5. `delete`

Deletes files or directories.

**Parameters:**
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to delete (relative to server root)"
    },
    "recursive": {
      "type": "boolean",
      "description": "Whether to recursively delete directories",
      "default": false
    },
    "force": {
      "type": "boolean",
      "description": "Force deletion even if errors occur",
      "default": false
    }
  },
  "required": ["path"]
}
```

**Response:**
```json
{
  "success": true,
  "path": "path/to/deleted_item",
  "type": "file"
}
```

### 6. `copy`

Copies files or directories.

**Parameters:**
```json
{
  "type": "object",
  "properties": {
    "source": {
      "type": "string",
      "description": "Source path (relative to server root)"
    },
    "destination": {
      "type": "string",
      "description": "Destination path (relative to server root)"
    },
    "overwrite": {
      "type": "boolean",
      "description": "Whether to overwrite existing files",
      "default": false
    },
    "recursive": {
      "type": "boolean",
      "description": "Whether to copy directories recursively",
      "default": true
    }
  },
  "required": ["source", "destination"]
}
```

**Response:**
```json
{
  "success": true,
  "source": "path/to/source",
  "destination": "path/to/destination",
  "bytes_copied": 5242880
}
```

### 7. `move`

Moves or renames files or directories.

**Parameters:**
```json
{
  "type": "object",
  "properties": {
    "source": {
      "type": "string",
      "description": "Source path (relative to server root)"
    },
    "destination": {
      "type": "string",
      "description": "Destination path (relative to server root)"
    },
    "overwrite": {
      "type": "boolean",
      "description": "Whether to overwrite existing files",
      "default": false
    }
  },
  "required": ["source", "destination"]
}
```

**Response:**
```json
{
  "success": true,
  "source": "path/to/source",
  "destination": "path/to/destination"
}
```

### 8. `info`

Gets detailed information about a file or directory.

**Parameters:**
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to get information for (relative to server root)"
    }
  },
  "required": ["path"]
}
```

**Response:**
```json
{
  "exists": true,
  "type": "file",
  "name": "example.txt",
  "path": "path/to/example.txt",
  "size": 1024,
  "created": "2024-04-09T12:00:00Z",
  "modified": "2024-04-10T15:30:00Z",
  "accessed": "2024-04-13T10:45:00Z", 
  "permissions": {
    "readable": true,
    "writable": true,
    "executable": false
  },
  "is_hidden": false
}
```

### 9. `search`

Searches file contents for matching patterns (grep-like functionality).

**Parameters:**
```json
{
  "type": "object",
  "properties": {
    "root_path": {
      "type": "string",
      "description": "Root directory to start the search from (relative to server root)"
    },
    "pattern": {
      "type": "string",
      "description": "Text pattern to search for in files"
    },
    "regex": {
      "type": "boolean",
      "description": "Whether to treat pattern as regex",
      "default": false
    },
    "file_pattern": {
      "type": "string",
      "description": "Optional glob pattern to filter which files to search",
      "default": "*"
    },
    "recursive": {
      "type": "boolean",
      "description": "Whether to search directories recursively",
      "default": true
    },
    "case_sensitive": {
      "type": "boolean",
      "description": "Whether the search should be case-sensitive",
      "default": false
    },
    "max_results": {
      "type": "integer",
      "description": "Maximum number of results to return",
      "default": 100
    },
    "max_file_size": {
      "type": "integer",
      "description": "Maximum file size to search (in bytes)",
      "default": 10485760
    },
    "context_lines": {
      "type": "integer",
      "description": "Number of context lines to include before and after matches",
      "default": 0
    }
  },
  "required": ["root_path", "pattern"]
}
```

**Response:**
```json
{
  "total_matches": 5,
  "files_searched": 25,
  "files_matched": 3,
  "matches": [
    {
      "file": "path/to/file1.txt",
      "matches": [
        {
          "line_number": 42,
          "line": "This is the matching line with the search pattern in it",
          "context": [
            {"line_number": 41, "content": "Line before match"},
            {"line_number": 42, "content": "This is the matching line with the search pattern in it"},
            {"line_number": 43, "content": "Line after match"}
          ]
        }
      ]
    },
    {
      "file": "path/to/file2.txt",
      "matches": [
        {
          "line_number": 10,
          "line": "Another matching line in a different file",
          "context": []
        }
      ]
    }
  ]
}
```

## Error Handling

All tools follow a consistent error handling pattern:

1. **Path Validation Errors**: When a path is outside the root directory or invalid
   - Error Code: `path_validation_error`
   - Message: "Path is outside of the allowed root directory"

2. **Permission Errors**: When file access permissions prevent an operation
   - Error Code: `permission_denied`
   - Message: "Permission denied to access the specified path"

3. **Not Found Errors**: When a specified file or directory doesn't exist
   - Error Code: `not_found`
   - Message: "The specified path does not exist"

4. **Already Exists Errors**: When trying to create a file that already exists
   - Error Code: `already_exists`
   - Message: "The file already exists and overwrite is not enabled"

5. **Invalid Parameters**: When provided parameters are invalid
   - Error Code: `invalid_parameters`
   - Message: Specific message about which parameter is invalid

Error responses will have the following format:

```json
{
  "error": {
    "code": "error_code",
    "message": "Human-readable error message",
    "details": {
      "path": "path/that/caused/error",
      "operation": "operation_that_failed"
    }
  }
}
```

## Configuration Options

The server supports the following configuration options:

1. **Root Directory**: Base directory for all operations
   - Environment Variable: `FS_SERVER_ROOT`
   - Command Line: `--root-dir <path>`

2. **Max File Size**: Maximum file size for read operations
   - Environment Variable: `FS_MAX_FILE_SIZE`
   - Command Line: `--max-file-size <bytes>`
   - Default: 10MB

3. **Request Timeout**: Maximum time for operations to complete
   - Environment Variable: `FS_REQUEST_TIMEOUT`
   - Command Line: `--request-timeout <seconds>`
   - Default: 30 seconds

4. **Logging Level**: Verbosity of server logs
   - Environment Variable: `FS_LOG_LEVEL`
   - Command Line: `--log-level <level>`
   - Options: error, warn, info, debug, trace
   - Default: info

## Implementation Details

1. **Path Handling**
   - All paths are validated against the server root
   - Symlinks are resolved to their canonical paths
   - Paths are normalized to handle different separators

2. **File Size Limits**
   - Large file reads are chunked
   - Files exceeding the size limit are truncated with a warning

3. **Binary vs Text**
   - Binary detection is performed for read operations
   - Binary files are automatically base64-encoded

4. **Search Performance**
   - File search uses memory-mapped files for large files
   - Binary files are skipped by default
   - Large directories are processed in parallel where possible

## Security Considerations

1. **Path Traversal Protection**
   - All paths are canonicalized before validation
   - No relative paths that escape the root are allowed

2. **Resource Limitations**
   - Maximum file size limits to prevent memory exhaustion
   - Request timeouts to prevent hanging operations
   - Rate limiting for high-frequency operations

3. **Permission Checking**
   - Server respects filesystem permissions
   - Validates write access before modification attempts

4. **Logging and Auditing**
   - All operations are logged with timestamps
   - Error conditions include detailed information for debugging

## Future Enhancements

1. **Watch Feature**: Tool to watch directories for changes
2. **File Streaming**: Support for streaming large files
3. **Compression**: Automatic compression/decompression of files
4. **Access Control Lists**: Fine-grained access control
5. **Content Type Detection**: MIME type detection for files
6. **Directory Diff**: Tool to compare directory structures
7. **File Fingerprinting**: Generate hash values for files
