//! Safe archive extraction for fixture releases (contract CF-06, decision D-06).
//!
//! Fixture release archives are `.tar.gz` packages containing fixture definitions,
//! payloads, and manifests. Untrusted archives are extracted only after verifying
//! that every entry conforms to lexical and structural limits:
//!
//! - Paths must be relative, valid UTF-8, using `/` as separator, with no leading
//!   `/`, no drive prefixes, no backslashes, no NUL or control characters, and no
//!   `.` or `..` segments (CF-02 / CF-06 path rules).
//! - Directory depth must not exceed [`MAX_PATH_DEPTH`] (8 segments).
//! - Only regular files and directories are allowed. Symlinks, hardlinks, fifos,
//!   and device nodes are rejected.
//! - Duplicate paths and paths colliding under Unicode case folding are rejected.
//! - Entry count must not exceed [`MAX_ENTRIES`] (2,000 entries).
//! - Single file size must not exceed [`MAX_FILE_BYTES`] (1 MiB).
//! - Total uncompressed size must not exceed [`MAX_TOTAL_BYTES`] (16 MiB).
//! - Setuid, setgid, or sticky mode bits are prohibited. On extraction, file
//!   permissions are normalized (e.g. 0644 for files, 0755 for directories).
//! - Files are created with `create_new` semantics so existing files or links are
//!   never overwritten or traversed.
//! - On any limit violation or format error, the extractor halts immediately,
//!   leaves nothing outside destination, and returns a detailed [`ArchiveSafetyError`].

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tar::{Archive, EntryType};

/// Most entries an archive may contain (decision D-06, CF-06 section 6).
pub const MAX_ENTRIES: usize = 2000;

/// Most bytes all of an archive's uncompressed entries may total (16 MiB, decision D-06, CF-06 section 6).
pub const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

/// Most uncompressed bytes one file may have (1 MiB, decision D-06, CF-06 section 6).
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// Deepest path allowed, counted in `/`-separated segments (decision D-06, CF-06 section 6).
pub const MAX_PATH_DEPTH: usize = 8;

/// Configurable resource limits for safe archive extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Maximum number of entries permitted.
    pub max_entries: usize,
    /// Maximum total uncompressed bytes across all entries.
    pub max_total_bytes: u64,
    /// Maximum uncompressed bytes for any single file.
    pub max_file_bytes: u64,
    /// Maximum directory depth in `/`-separated segments.
    pub max_path_depth: usize,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_entries: MAX_ENTRIES,
            max_total_bytes: MAX_TOTAL_BYTES,
            max_file_bytes: MAX_FILE_BYTES,
            max_path_depth: MAX_PATH_DEPTH,
        }
    }
}

/// Summary of a successful archive extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionSummary {
    /// Number of entries successfully extracted.
    pub entries_extracted: usize,
    /// Total uncompressed bytes written to destination.
    pub total_bytes: u64,
    /// List of entry paths extracted, normalized relative to destination.
    pub files: Vec<String>,
}

/// Errors returned when an archive violates safety boundaries or resource limits.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveSafetyError {
    #[error("failed to decompress gzip archive: {0}")]
    InvalidGzip(#[source] std::io::Error),

    #[error("failed to parse tar archive: {0}")]
    InvalidTar(#[source] std::io::Error),

    #[error("path traversal detected: {path:?}")]
    PathTraversal { path: String },

    #[error("absolute path rejected: {path:?}")]
    AbsolutePath { path: String },

    #[error("invalid archive path {path:?}: {reason}")]
    InvalidPath { path: String, reason: &'static str },

    #[error("unsupported entry type {entry_type:?} for {path:?}; only regular files and directories are allowed")]
    UnsupportedEntryType { path: String, entry_type: String },

    #[error("archive contains duplicate path: {path:?}")]
    DuplicatePath { path: String },

    #[error("archive paths {first:?} and {second:?} collide under Unicode case folding")]
    CaseCollision { first: String, second: String },

    #[error("archive entry count {count} exceeds maximum limit of {limit}")]
    EntryLimitExceeded { count: usize, limit: usize },

    #[error(
        "entry {path:?} exceeds single file size limit ({size} bytes, limit is {limit} bytes)"
    )]
    FileSizeLimitExceeded { path: String, size: u64, limit: u64 },

    #[error(
        "archive total uncompressed size exceeds limit ({bytes} bytes, limit is {limit} bytes)"
    )]
    TotalSizeLimitExceeded { bytes: u64, limit: u64 },

    #[error("entry {path:?} path depth {depth} exceeds maximum limit of {limit} segments")]
    DepthLimitExceeded {
        path: String,
        depth: usize,
        limit: usize,
    },

    #[error("prohibited mode bits 0o{mode:o} on entry {path:?} (setuid, setgid, or sticky bits not allowed)")]
    ProhibitedModeBits { path: String, mode: u32 },

    #[error("destination path {path:?} is not a directory: {reason}")]
    DestinationError { path: PathBuf, reason: String },

    #[error("I/O error during extraction at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl From<std::io::Error> for ArchiveSafetyError {
    fn from(err: std::io::Error) -> Self {
        map_io_error(err)
    }
}

fn map_io_error(e: std::io::Error) -> ArchiveSafetyError {
    let msg = e.to_string().to_lowercase();
    if msg.contains("gzip")
        || msg.contains("corrupt deflate stream")
        || msg.contains("invalid header")
    {
        ArchiveSafetyError::InvalidGzip(e)
    } else {
        ArchiveSafetyError::InvalidTar(e)
    }
}

/// Unicode case folding, identical to CF-02 (`digest::case_fold`).
fn case_fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_uppercase)
        .flat_map(char::to_lowercase)
        .collect()
}

/// Safely extracts a `.tar.gz` archive to `dest` under default limits (decision D-06).
pub fn extract_archive<R: Read>(
    reader: R,
    dest: &Path,
) -> Result<ExtractionSummary, ArchiveSafetyError> {
    extract_archive_with_limits(reader, dest, &ArchiveLimits::default())
}

/// Safely extracts a `.tar.gz` archive to `dest` with custom [`ArchiveLimits`].
pub fn extract_archive_with_limits<R: Read>(
    reader: R,
    dest: &Path,
    limits: &ArchiveLimits,
) -> Result<ExtractionSummary, ArchiveSafetyError> {
    // Verify destination directory
    match std::fs::symlink_metadata(dest) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(ArchiveSafetyError::DestinationError {
                    path: dest.to_path_buf(),
                    reason: "destination is a symbolic link".to_string(),
                });
            }
            if !meta.is_dir() {
                return Err(ArchiveSafetyError::DestinationError {
                    path: dest.to_path_buf(),
                    reason: "destination is not a directory".to_string(),
                });
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(dest).map_err(|e| ArchiveSafetyError::Io {
                path: dest.to_path_buf(),
                source: e,
            })?;
        }
        Err(e) => {
            return Err(ArchiveSafetyError::Io {
                path: dest.to_path_buf(),
                source: e,
            });
        }
    }

    // Entries already in `dest` are never ours to delete: remember them so a
    // failed extraction removes only what it created.
    let preexisting: HashSet<std::ffi::OsString> = std::fs::read_dir(dest)
        .map(|entries| entries.flatten().map(|entry| entry.file_name()).collect())
        .unwrap_or_default();

    let result = perform_extraction(reader, dest, limits);
    if result.is_err() {
        // Safe failure behavior: leave no partially extracted output behind.
        if let Ok(entries) = std::fs::read_dir(dest) {
            for entry in entries.flatten() {
                if preexisting.contains(&entry.file_name()) {
                    continue;
                }
                let path = entry.path();
                let _ = match entry.file_type() {
                    Ok(kind) if kind.is_dir() => std::fs::remove_dir_all(&path),
                    _ => std::fs::remove_file(&path),
                };
            }
        }
    }
    result
}

fn perform_extraction<R: Read>(
    reader: R,
    dest: &Path,
    limits: &ArchiveLimits,
) -> Result<ExtractionSummary, ArchiveSafetyError> {
    let gz = GzDecoder::new(reader);
    let mut archive = Archive::new(gz);

    let entries = archive.entries().map_err(map_io_error)?;

    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut case_folded_paths: HashMap<String, String> = HashMap::new();
    let mut extracted_files: Vec<String> = Vec::new();
    let mut entry_count: usize = 0;
    let mut total_uncompressed_bytes: u64 = 0;

    for entry in entries {
        let mut entry = entry.map_err(map_io_error)?;

        entry_count += 1;
        if entry_count > limits.max_entries {
            return Err(ArchiveSafetyError::EntryLimitExceeded {
                count: entry_count,
                limit: limits.max_entries,
            });
        }

        // Validate raw path bytes
        let raw_bytes = entry.path_bytes().to_vec();
        if raw_bytes.contains(&0) {
            return Err(ArchiveSafetyError::InvalidPath {
                path: String::from_utf8_lossy(&raw_bytes).into_owned(),
                reason: "contains a NUL byte",
            });
        }

        let path_str = match String::from_utf8(raw_bytes.clone()) {
            Ok(s) => s,
            Err(_) => {
                return Err(ArchiveSafetyError::InvalidPath {
                    path: String::from_utf8_lossy(&raw_bytes).into_owned(),
                    reason: "is not valid UTF-8",
                });
            }
        };

        if path_str.is_empty() {
            return Err(ArchiveSafetyError::InvalidPath {
                path: path_str.to_string(),
                reason: "path is empty",
            });
        }

        if path_str.contains('\\') {
            return Err(ArchiveSafetyError::InvalidPath {
                path: path_str.to_string(),
                reason: "contains a backslash",
            });
        }

        if path_str.chars().any(char::is_control) {
            return Err(ArchiveSafetyError::InvalidPath {
                path: path_str.to_string(),
                reason: "contains a control character",
            });
        }

        if path_str.starts_with('/') {
            return Err(ArchiveSafetyError::AbsolutePath {
                path: path_str.to_string(),
            });
        }

        let bytes = path_str.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return Err(ArchiveSafetyError::AbsolutePath {
                path: path_str.to_string(),
            });
        }

        let is_dir_by_path = path_str.ends_with('/');
        let normalized = path_str.trim_end_matches('/');
        if normalized.is_empty() {
            return Err(ArchiveSafetyError::AbsolutePath {
                path: path_str.to_string(),
            });
        }

        let segments: Vec<&str> = normalized.split('/').collect();
        for segment in &segments {
            if *segment == ".." {
                return Err(ArchiveSafetyError::PathTraversal {
                    path: path_str.to_string(),
                });
            }
            if *segment == "." {
                return Err(ArchiveSafetyError::InvalidPath {
                    path: path_str.to_string(),
                    reason: "contains a '.' segment",
                });
            }
            if segment.is_empty() {
                return Err(ArchiveSafetyError::InvalidPath {
                    path: path_str.to_string(),
                    reason: "contains an empty segment",
                });
            }
        }

        if segments.len() > limits.max_path_depth {
            return Err(ArchiveSafetyError::DepthLimitExceeded {
                path: path_str.to_string(),
                depth: segments.len(),
                limit: limits.max_path_depth,
            });
        }

        // Validate entry type
        let entry_type = entry.header().entry_type();
        let is_dir = entry_type.is_dir() || is_dir_by_path;
        let is_file = entry_type.is_file() || entry_type == EntryType::Continuous;

        if !is_dir && !is_file {
            let type_desc = match entry_type {
                EntryType::Symlink => "symbolic link",
                EntryType::Link => "hard link",
                EntryType::Char => "character device",
                EntryType::Block => "block device",
                EntryType::Fifo => "fifo",
                _ => "unsupported special entry",
            };
            return Err(ArchiveSafetyError::UnsupportedEntryType {
                path: path_str,
                entry_type: type_desc.to_string(),
            });
        }

        // Validate mode bits
        let mode = entry.header().mode().map_err(map_io_error)?;
        const S_ISUID: u32 = 0o4000;
        const S_ISGID: u32 = 0o2000;
        const S_ISVTX: u32 = 0o1000;
        if mode & (S_ISUID | S_ISGID | S_ISVTX) != 0 {
            return Err(ArchiveSafetyError::ProhibitedModeBits {
                path: path_str.to_string(),
                mode,
            });
        }

        // Duplicate and case folding checks
        let path_key = normalized.to_string();
        if seen_paths.contains(&path_key) {
            return Err(ArchiveSafetyError::DuplicatePath {
                path: path_str.to_string(),
            });
        }

        let folded = case_fold(&path_key);
        if let Some(first) = case_folded_paths.get(&folded) {
            return Err(ArchiveSafetyError::CaseCollision {
                first: first.clone(),
                second: path_str.to_string(),
            });
        }
        seen_paths.insert(path_key.clone());
        case_folded_paths.insert(folded, path_str.to_string());

        if is_dir {
            let dir_path = dest.join(normalized);
            if !dir_path.exists() {
                std::fs::create_dir_all(&dir_path).map_err(|e| ArchiveSafetyError::Io {
                    path: dir_path.clone(),
                    source: e,
                })?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&dir_path, std::fs::Permissions::from_mode(0o755));
                }
            }
            extracted_files.push(path_str.to_string());
            continue;
        }

        // Regular file checks
        let header_size = entry.header().size().map_err(map_io_error)?;
        if header_size > limits.max_file_bytes {
            return Err(ArchiveSafetyError::FileSizeLimitExceeded {
                path: path_str.to_string(),
                size: header_size,
                limit: limits.max_file_bytes,
            });
        }

        let file_path = dest.join(normalized);
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| ArchiveSafetyError::Io {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }
        }

        let mut out_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file_path)
            .map_err(|e| ArchiveSafetyError::Io {
                path: file_path.clone(),
                source: e,
            })?;

        let mut file_bytes: u64 = 0;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = match entry.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return Err(map_io_error(e)),
            };
            file_bytes += n as u64;
            if file_bytes > limits.max_file_bytes {
                return Err(ArchiveSafetyError::FileSizeLimitExceeded {
                    path: path_str.to_string(),
                    size: file_bytes,
                    limit: limits.max_file_bytes,
                });
            }
            total_uncompressed_bytes += n as u64;
            if total_uncompressed_bytes > limits.max_total_bytes {
                return Err(ArchiveSafetyError::TotalSizeLimitExceeded {
                    bytes: total_uncompressed_bytes,
                    limit: limits.max_total_bytes,
                });
            }
            out_file
                .write_all(&buf[..n])
                .map_err(|e| ArchiveSafetyError::Io {
                    path: file_path.clone(),
                    source: e,
                })?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o644));
        }

        extracted_files.push(path_str.to_string());
    }

    Ok(ExtractionSummary {
        entries_extracted: entry_count,
        total_bytes: total_uncompressed_bytes,
        files: extracted_files,
    })
}
