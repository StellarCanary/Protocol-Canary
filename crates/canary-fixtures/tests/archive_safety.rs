//! Integration tests and synthetic negative archive corpus for safe archive extraction.
//!
//! Validates CF-06 section 6 and decision D-06 security boundaries:
//! - Path traversal (`../escape.txt`, nested `..`)
//! - Absolute paths (`/root.txt`, `C:/root.txt`)
//! - CF-02 lexical rules (backslashes, NUL bytes, control characters, empty segments, dot segments)
//! - Non-regular entries (symlink escapes, hard links, devices, fifos)
//! - Duplicate paths and Unicode case-folding collisions
//! - Resource limits: single-file (> 1 MiB), total uncompressed (> 16 MiB), entry count (> 2,000), path depth (> 8)
//! - Prohibited mode bits (setuid, setgid, sticky)
//! - Corrupted / truncated gzip and tar streams
//! - Invariant: nothing is ever written outside destination

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_fixtures::{
    extract_archive, extract_archive_with_limits, ArchiveLimits, ArchiveSafetyError, MAX_ENTRIES,
    MAX_FILE_BYTES, MAX_PATH_DEPTH, MAX_TOTAL_BYTES,
};
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::{Builder, EntryType, Header};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Test isolation sandbox with sentinel verification outside the extraction target.
struct TestSandbox {
    dir: PathBuf,
    dest: PathBuf,
    sentinel: PathBuf,
}

impl TestSandbox {
    fn new(prefix: &str) -> Self {
        let mut dir = std::env::temp_dir();
        let unique = format!(
            "canary-archive-{prefix}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        dir.push(unique);
        std::fs::create_dir_all(&dir).unwrap();

        let dest = dir.join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        let sentinel = dir.join("outside_sentinel.txt");
        std::fs::write(&sentinel, "original sentinel untouched").unwrap();

        Self {
            dir,
            dest,
            sentinel,
        }
    }

    /// Asserts that no operation wrote or altered files outside `dest`.
    fn assert_nothing_written_outside(&self) {
        // Sentinel must remain unchanged
        let sentinel_content = std::fs::read_to_string(&self.sentinel)
            .expect("sentinel file was deleted or unreadable");
        assert_eq!(
            sentinel_content, "original sentinel untouched",
            "Sentinel outside destination was modified!"
        );

        // Parent dir must only contain `dest` and `outside_sentinel.txt`
        let entries = std::fs::read_dir(&self.dir).unwrap();
        for entry in entries {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name_str = name.to_str().unwrap();
            assert!(
                name_str == "dest" || name_str == "outside_sentinel.txt",
                "Unexpected file or directory leaked outside destination: {name_str}"
            );
        }
    }
}

impl Drop for TestSandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Entry descriptor for synthetic archive generation.
struct SyntheticEntry {
    path: String,
    data: Vec<u8>,
    entry_type: EntryType,
    mode: u32,
    link_name: Option<String>,
    header_size_override: Option<u64>,
    raw_path_bytes: Option<Vec<u8>>,
}

/// Fluent builder for synthetic in-memory `.tar.gz` and `.tar` archives.
struct ArchiveFixtureBuilder {
    entries: Vec<SyntheticEntry>,
}

impl ArchiveFixtureBuilder {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn file(mut self, path: &str, data: &[u8]) -> Self {
        self.entries.push(SyntheticEntry {
            path: path.to_string(),
            data: data.to_vec(),
            entry_type: EntryType::Regular,
            mode: 0o644,
            link_name: None,
            header_size_override: None,
            raw_path_bytes: None,
        });
        self
    }

    fn file_with_mode(mut self, path: &str, data: &[u8], mode: u32) -> Self {
        self.entries.push(SyntheticEntry {
            path: path.to_string(),
            data: data.to_vec(),
            entry_type: EntryType::Regular,
            mode,
            link_name: None,
            header_size_override: None,
            raw_path_bytes: None,
        });
        self
    }

    fn file_with_header_size(mut self, path: &str, data: &[u8], header_size: u64) -> Self {
        self.entries.push(SyntheticEntry {
            path: path.to_string(),
            data: data.to_vec(),
            entry_type: EntryType::Regular,
            mode: 0o644,
            link_name: None,
            header_size_override: Some(header_size),
            raw_path_bytes: None,
        });
        self
    }

    fn directory(mut self, path: &str) -> Self {
        self.entries.push(SyntheticEntry {
            path: path.to_string(),
            data: Vec::new(),
            entry_type: EntryType::Directory,
            mode: 0o755,
            link_name: None,
            header_size_override: None,
            raw_path_bytes: None,
        });
        self
    }

    fn symlink(mut self, path: &str, link_target: &str) -> Self {
        self.entries.push(SyntheticEntry {
            path: path.to_string(),
            data: Vec::new(),
            entry_type: EntryType::Symlink,
            mode: 0o777,
            link_name: Some(link_target.to_string()),
            header_size_override: None,
            raw_path_bytes: None,
        });
        self
    }

    fn hardlink(mut self, path: &str, link_target: &str) -> Self {
        self.entries.push(SyntheticEntry {
            path: path.to_string(),
            data: Vec::new(),
            entry_type: EntryType::Link,
            mode: 0o644,
            link_name: Some(link_target.to_string()),
            header_size_override: None,
            raw_path_bytes: None,
        });
        self
    }

    fn fifo(mut self, path: &str) -> Self {
        self.entries.push(SyntheticEntry {
            path: path.to_string(),
            data: Vec::new(),
            entry_type: EntryType::Fifo,
            mode: 0o644,
            link_name: None,
            header_size_override: None,
            raw_path_bytes: None,
        });
        self
    }

    fn char_device(mut self, path: &str) -> Self {
        self.entries.push(SyntheticEntry {
            path: path.to_string(),
            data: Vec::new(),
            entry_type: EntryType::Char,
            mode: 0o644,
            link_name: None,
            header_size_override: None,
            raw_path_bytes: None,
        });
        self
    }

    fn block_device(mut self, path: &str) -> Self {
        self.entries.push(SyntheticEntry {
            path: path.to_string(),
            data: Vec::new(),
            entry_type: EntryType::Block,
            mode: 0o644,
            link_name: None,
            header_size_override: None,
            raw_path_bytes: None,
        });
        self
    }

    fn raw_path_entry(mut self, raw_bytes: &[u8], data: &[u8]) -> Self {
        self.entries.push(SyntheticEntry {
            path: String::from_utf8_lossy(raw_bytes).into_owned(),
            data: data.to_vec(),
            entry_type: EntryType::Regular,
            mode: 0o644,
            link_name: None,
            header_size_override: None,
            raw_path_bytes: Some(raw_bytes.to_vec()),
        });
        self
    }

    /// Builds an uncompressed `.tar` byte vector.
    fn build_tar(self) -> Vec<u8> {
        let mut builder = Builder::new(Vec::new());
        for entry in self.entries {
            if let Some(mut raw) = entry.raw_path_bytes {
                let mut long_header = Header::new_gnu();
                long_header.set_entry_type(EntryType::new(b'L'));
                long_header.set_mode(0o644);
                raw.push(0);
                long_header.set_size(raw.len() as u64);
                let _ = long_header.set_path(Path::new("././@LongLink"));
                long_header.set_cksum();
                builder.append(&long_header, &raw[..]).unwrap();

                let mut header = Header::new_gnu();
                header.set_entry_type(entry.entry_type);
                header.set_mode(entry.mode);
                let size = entry
                    .header_size_override
                    .unwrap_or(entry.data.len() as u64);
                header.set_size(size);
                let _ = header.set_path(Path::new("placeholder.txt"));
                header.set_cksum();
                builder.append(&header, &entry.data[..]).unwrap();
                continue;
            }

            let mut header = Header::new_gnu();
            header.set_entry_type(entry.entry_type);
            header.set_mode(entry.mode);
            let size = entry
                .header_size_override
                .unwrap_or(entry.data.len() as u64);
            header.set_size(size);

            if let Some(target) = &entry.link_name {
                let target_bytes = target.as_bytes();
                let link_slice = &mut header.as_mut_bytes()[157..257];
                link_slice.fill(0);
                let len = target_bytes.len().min(100);
                link_slice[..len].copy_from_slice(&target_bytes[..len]);
            }

            let path_bytes = entry.path.into_bytes();
            let header_bytes = header.as_mut_bytes();
            header_bytes[..100].fill(0);
            let len = path_bytes.len().min(100);
            header_bytes[..len].copy_from_slice(&path_bytes[..len]);

            header.set_cksum();
            builder.append(&header, &entry.data[..]).unwrap();
        }
        builder.into_inner().unwrap()
    }

    /// Builds a `.tar.gz` compressed byte vector.
    fn build(self) -> Vec<u8> {
        let tar_bytes = self.build_tar();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        std::io::Write::write_all(&mut encoder, &tar_bytes).unwrap();
        encoder.finish().unwrap()
    }
}

// ============================================================================
// 1. Path Traversal Tests
// ============================================================================

#[test]
fn negative_path_traversal_leading_parent() {
    let sandbox = TestSandbox::new("traversal-leading");
    let archive = ArchiveFixtureBuilder::new()
        .file("../escape.txt", b"malicious content outside destination")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::PathTraversal { path }) => {
            assert!(path.contains(".."));
        }
        other => panic!("Expected PathTraversal error, got {other:?}"),
    }

    sandbox.assert_nothing_written_outside();
    assert!(!sandbox.dir.join("escape.txt").exists());
}

#[test]
fn negative_path_traversal_nested_parent() {
    let sandbox = TestSandbox::new("traversal-nested");
    let archive = ArchiveFixtureBuilder::new()
        .file("safe_dir/../../escape.txt", b"evil traversal")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::PathTraversal { path }) => {
            assert!(path.contains(".."));
        }
        other => panic!("Expected PathTraversal error, got {other:?}"),
    }

    sandbox.assert_nothing_written_outside();
    assert!(!sandbox.dir.join("escape.txt").exists());
}

#[test]
fn negative_path_traversal_trailing_parent() {
    let sandbox = TestSandbox::new("traversal-trailing");
    let archive = ArchiveFixtureBuilder::new()
        .file("dir/escape.txt/..", b"evil")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    assert!(matches!(
        result,
        Err(ArchiveSafetyError::PathTraversal { .. })
    ));
    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 2. Absolute Path Tests
// ============================================================================

#[test]
fn negative_absolute_path_unix_root() {
    let sandbox = TestSandbox::new("abs-root");
    let archive = ArchiveFixtureBuilder::new()
        .file("/root.txt", b"absolute path attempt")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::AbsolutePath { path }) => {
            assert!(path.starts_with('/'));
        }
        other => panic!("Expected AbsolutePath error, got {other:?}"),
    }

    sandbox.assert_nothing_written_outside();
    assert!(!Path::new("/root.txt").exists());
}

#[test]
fn negative_absolute_path_windows_drive() {
    let sandbox = TestSandbox::new("abs-windows");
    let archive = ArchiveFixtureBuilder::new()
        .file("C:/root.txt", b"windows drive letter attempt")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    assert!(matches!(
        result,
        Err(ArchiveSafetyError::AbsolutePath { .. })
    ));
    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 3. CF-02 Lexical Path Validation (Backslashes, NUL, Control, Dot Segments)
// ============================================================================

#[test]
fn negative_path_with_backslash() {
    let sandbox = TestSandbox::new("backslash");
    let archive = ArchiveFixtureBuilder::new()
        .file("sub\\escape.txt", b"backslash traversal attempt")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::InvalidPath { reason, .. }) => {
            assert!(reason.contains("backslash"));
        }
        other => panic!("Expected InvalidPath with backslash, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_path_with_nul_byte() {
    let sandbox = TestSandbox::new("nul-byte");
    let archive = ArchiveFixtureBuilder::new()
        .raw_path_entry(b"fixtures/bad\0name.toml", b"nul byte in name")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::InvalidPath { reason, .. }) => {
            assert!(reason.contains("NUL byte"));
        }
        other => panic!("Expected InvalidPath with NUL byte, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_path_with_control_character() {
    let sandbox = TestSandbox::new("control-char");
    let archive = ArchiveFixtureBuilder::new()
        .file("dir/control\x07bell.txt", b"bell character")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::InvalidPath { reason, .. }) => {
            assert!(reason.contains("control character"));
        }
        other => panic!("Expected InvalidPath for control character, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_path_with_empty_segment() {
    let sandbox = TestSandbox::new("empty-segment");
    let archive = ArchiveFixtureBuilder::new()
        .file("dir//file.txt", b"empty segment")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::InvalidPath { reason, .. }) => {
            assert!(reason.contains("empty segment"));
        }
        other => panic!("Expected InvalidPath for empty segment, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_path_with_dot_segment() {
    let sandbox = TestSandbox::new("dot-segment");
    let archive = ArchiveFixtureBuilder::new()
        .file("dir/./file.txt", b"dot segment")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::InvalidPath { reason, .. }) => {
            assert!(reason.contains("'.' segment"));
        }
        other => panic!("Expected InvalidPath for dot segment, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_path_with_leading_dot() {
    let sandbox = TestSandbox::new("leading-dot");
    let archive = ArchiveFixtureBuilder::new()
        .file("./file.txt", b"leading dot")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::InvalidPath { reason, .. }) => {
            assert!(reason.contains("'.' segment"));
        }
        other => panic!("Expected InvalidPath for leading dot, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 4. Unsupported Entry Types (Symlinks, Hardlinks, Special Devices)
// ============================================================================

#[test]
fn negative_symlink_escape_rejected() {
    let sandbox = TestSandbox::new("symlink-escape");
    let archive = ArchiveFixtureBuilder::new()
        .symlink("link_escape", "../outside_sentinel.txt")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::UnsupportedEntryType { entry_type, .. }) => {
            assert!(entry_type.contains("symbolic link"));
        }
        other => panic!("Expected UnsupportedEntryType for symlink, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_hardlink_rejected() {
    let sandbox = TestSandbox::new("hardlink");
    let archive = ArchiveFixtureBuilder::new()
        .file("target.txt", b"original")
        .hardlink("linked.txt", "target.txt")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::UnsupportedEntryType { entry_type, .. }) => {
            assert!(entry_type.contains("hard link"));
        }
        other => panic!("Expected UnsupportedEntryType for hard link, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_fifo_rejected() {
    let sandbox = TestSandbox::new("fifo");
    let archive = ArchiveFixtureBuilder::new().fifo("pipe.fifo").build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::UnsupportedEntryType { entry_type, .. }) => {
            assert!(entry_type.contains("fifo"));
        }
        other => panic!("Expected UnsupportedEntryType for fifo, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_char_device_rejected() {
    let sandbox = TestSandbox::new("char-device");
    let archive = ArchiveFixtureBuilder::new().char_device("null_dev").build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    assert!(matches!(
        result,
        Err(ArchiveSafetyError::UnsupportedEntryType { .. })
    ));
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_block_device_rejected() {
    let sandbox = TestSandbox::new("block-device");
    let archive = ArchiveFixtureBuilder::new().block_device("sda").build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    assert!(matches!(
        result,
        Err(ArchiveSafetyError::UnsupportedEntryType { .. })
    ));
    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 5. Duplicate Paths and Case Collisions
// ============================================================================

#[test]
fn negative_duplicate_entry_path() {
    let sandbox = TestSandbox::new("duplicate");
    let archive = ArchiveFixtureBuilder::new()
        .file("fixtures/fixture1.toml", b"first version")
        .file("fixtures/fixture1.toml", b"second version")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::DuplicatePath { path }) => {
            assert_eq!(path, "fixtures/fixture1.toml");
        }
        other => panic!("Expected DuplicatePath error, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_case_collision_entry_path() {
    let sandbox = TestSandbox::new("case-collision");
    let archive = ArchiveFixtureBuilder::new()
        .file("fixtures/test.toml", b"lower")
        .file("fixtures/TEST.TOML", b"upper")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::CaseCollision { first, second }) => {
            assert_eq!(first, "fixtures/test.toml");
            assert_eq!(second, "fixtures/TEST.TOML");
        }
        other => panic!("Expected CaseCollision error, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_case_collision_unicode() {
    let sandbox = TestSandbox::new("case-unicode");
    let archive = ArchiveFixtureBuilder::new()
        .file("fixtures/groß.toml", b"sharp s")
        .file("fixtures/GROSS.TOML", b"double s")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    assert!(matches!(
        result,
        Err(ArchiveSafetyError::CaseCollision { .. })
    ));
    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 6. Oversized Single File (> 1 MiB)
// ============================================================================

#[test]
fn negative_oversized_file_header_limit() {
    let sandbox = TestSandbox::new("oversize-header");
    // Header claims 1 byte over 1 MiB limit
    let archive = ArchiveFixtureBuilder::new()
        .file_with_header_size("large.bin", b"small payload", MAX_FILE_BYTES + 1)
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::FileSizeLimitExceeded { path, size, limit }) => {
            assert_eq!(path, "large.bin");
            assert_eq!(size, MAX_FILE_BYTES + 1);
            assert_eq!(limit, MAX_FILE_BYTES);
        }
        other => panic!("Expected FileSizeLimitExceeded, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_oversized_file_streamed_limit() {
    let sandbox = TestSandbox::new("oversize-stream");
    // Generate actual stream payload of 1 MiB + 1 byte (1,048,577 bytes)
    // Highly compressible (zeros) so the archive remains a few hundred bytes in memory
    let big_data = vec![0u8; (MAX_FILE_BYTES + 1) as usize];
    let archive = ArchiveFixtureBuilder::new()
        .file("bomb.bin", &big_data)
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::FileSizeLimitExceeded { path, limit, .. }) => {
            assert_eq!(path, "bomb.bin");
            assert_eq!(limit, MAX_FILE_BYTES);
        }
        other => panic!("Expected FileSizeLimitExceeded on streamed data, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 7. Oversized Total Uncompressed Size (> 16 MiB) & Zip Bomb Protection
// ============================================================================

#[test]
fn negative_oversized_total_uncompressed_bytes() {
    let sandbox = TestSandbox::new("oversize-total");
    // 17 files, each 1 MiB of zeros (individually <= 1 MiB, but total = 17 MiB > 16 MiB).
    // In gzip, 17 MiB of zeros compresses to ~17 KiB.
    let one_mib = vec![0u8; MAX_FILE_BYTES as usize];
    let mut builder = ArchiveFixtureBuilder::new();
    for i in 0..17 {
        builder = builder.file(&format!("chunk_{i:02}.bin"), &one_mib);
    }
    let archive = builder.build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::TotalSizeLimitExceeded { bytes, limit }) => {
            assert!(bytes > MAX_TOTAL_BYTES);
            assert_eq!(limit, MAX_TOTAL_BYTES);
        }
        other => panic!("Expected TotalSizeLimitExceeded, got {other:?}"),
    }

    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 8. Excessive Entry Count (> 2,000 Entries)
// ============================================================================

#[test]
fn negative_excessive_entry_count() {
    let sandbox = TestSandbox::new("excessive-entries");
    // 2,001 tiny entries
    let mut builder = ArchiveFixtureBuilder::new();
    for i in 0..=MAX_ENTRIES {
        builder = builder.file(&format!("file_{i}.txt"), b"x");
    }
    let archive = builder.build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::EntryLimitExceeded { count, limit }) => {
            assert_eq!(count, MAX_ENTRIES + 1);
            assert_eq!(limit, MAX_ENTRIES);
        }
        other => panic!("Expected EntryLimitExceeded, got {other:?}"),
    }

    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 9. Excessive Path Depth (> 8 Segments)
// ============================================================================

#[test]
fn negative_excessive_path_depth() {
    let sandbox = TestSandbox::new("excessive-depth");
    // 9 segments: 1/2/3/4/5/6/7/8/9.txt
    let deep_path = "1/2/3/4/5/6/7/8/9.txt";
    let archive = ArchiveFixtureBuilder::new()
        .file(deep_path, b"too deep")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::DepthLimitExceeded { path, depth, limit }) => {
            assert_eq!(path, deep_path);
            assert_eq!(depth, 9);
            assert_eq!(limit, MAX_PATH_DEPTH);
        }
        other => panic!("Expected DepthLimitExceeded, got {other:?}"),
    }

    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 10. Prohibited Mode Bits (Setuid, Setgid, Sticky)
// ============================================================================

#[test]
fn negative_prohibited_mode_setuid() {
    let sandbox = TestSandbox::new("mode-setuid");
    let archive = ArchiveFixtureBuilder::new()
        .file_with_mode("exploit.sh", b"echo pwn", 0o4755)
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    match result {
        Err(ArchiveSafetyError::ProhibitedModeBits { path, mode }) => {
            assert_eq!(path, "exploit.sh");
            assert_ne!(mode & 0o4000, 0);
        }
        other => panic!("Expected ProhibitedModeBits for setuid, got {other:?}"),
    }
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_prohibited_mode_setgid() {
    let sandbox = TestSandbox::new("mode-setgid");
    let archive = ArchiveFixtureBuilder::new()
        .file_with_mode("exploit.sh", b"echo pwn", 0o2755)
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    assert!(matches!(
        result,
        Err(ArchiveSafetyError::ProhibitedModeBits { .. })
    ));
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_prohibited_mode_sticky() {
    let sandbox = TestSandbox::new("mode-sticky");
    let archive = ArchiveFixtureBuilder::new()
        .file_with_mode("exploit.sh", b"echo pwn", 0o1755)
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    assert!(matches!(
        result,
        Err(ArchiveSafetyError::ProhibitedModeBits { .. })
    ));
    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 11. Corrupted and Truncated Archives
// ============================================================================

#[test]
fn negative_corrupted_gzip_header() {
    let sandbox = TestSandbox::new("corrupt-gzip");
    let invalid_bytes = b"THIS IS DEFINITELY NOT GZIP DATA 1234567890";

    let result = extract_archive(&invalid_bytes[..], &sandbox.dest);
    assert!(matches!(result, Err(ArchiveSafetyError::InvalidGzip(_))));
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_truncated_gzip_stream() {
    let sandbox = TestSandbox::new("truncated-gzip");
    let valid_archive = ArchiveFixtureBuilder::new()
        .file("file1.txt", b"hello world")
        .build();

    // Truncate halfway through compressed stream
    let truncated = &valid_archive[..valid_archive.len() / 2];

    let result = extract_archive(truncated, &sandbox.dest);
    assert!(result.is_err());
    sandbox.assert_nothing_written_outside();
}

#[test]
fn negative_corrupted_tar_in_valid_gzip() {
    let sandbox = TestSandbox::new("corrupt-tar");
    // Compress non-tar data with valid gzip
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut encoder, b"valid gzip holding corrupted non-tar bytes").unwrap();
    let archive = encoder.finish().unwrap();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    assert!(matches!(result, Err(ArchiveSafetyError::InvalidTar(_))));
    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 12. Positive Control Test (Roundtrip Extraction)
// ============================================================================

#[test]
fn positive_control_valid_archive_roundtrip() {
    let sandbox = TestSandbox::new("positive-control");
    let archive = ArchiveFixtureBuilder::new()
        .directory("fixtures")
        .file("fixtures/fixture1.toml", b"id = \"fixture-1\"\n")
        .file("fixtures/fixture2.toml", b"id = \"fixture-2\"\n")
        .file("README.md", b"# Fixtures\n")
        .build();

    let summary = extract_archive(archive.as_slice(), &sandbox.dest).unwrap();
    assert_eq!(summary.entries_extracted, 4);
    assert!(summary.total_bytes > 0);

    // Verify files exist on disk
    let file1 = sandbox.dest.join("fixtures/fixture1.toml");
    let file2 = sandbox.dest.join("fixtures/fixture2.toml");
    let readme = sandbox.dest.join("README.md");

    assert!(file1.exists());
    assert!(file2.exists());
    assert!(readme.exists());

    assert_eq!(
        std::fs::read_to_string(file1).unwrap(),
        "id = \"fixture-1\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(file2).unwrap(),
        "id = \"fixture-2\"\n"
    );
    assert_eq!(std::fs::read_to_string(readme).unwrap(), "# Fixtures\n");

    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 13. Custom Limits Verification
// ============================================================================

#[test]
fn custom_limits_enforced() {
    let sandbox = TestSandbox::new("custom-limits");
    let limits = ArchiveLimits {
        max_entries: 5,
        max_total_bytes: 100,
        max_file_bytes: 50,
        max_path_depth: 3,
    };

    let archive = ArchiveFixtureBuilder::new()
        .file("1/2/3/4/deep.txt", b"too deep for custom limits")
        .build();

    let result = extract_archive_with_limits(archive.as_slice(), &sandbox.dest, &limits);
    match result {
        Err(ArchiveSafetyError::DepthLimitExceeded { depth, limit, .. }) => {
            assert_eq!(depth, 5);
            assert_eq!(limit, 3);
        }
        other => panic!("Expected DepthLimitExceeded, got {other:?}"),
    }

    sandbox.assert_nothing_written_outside();
}

// ============================================================================
// 14. Failed extraction leaves the destination as it found it
// ============================================================================

#[test]
fn failed_extraction_removes_what_it_wrote() {
    let sandbox = TestSandbox::new("cleanup-partial");
    // The first two entries are valid and are written before the symlink is
    // rejected, so there is partial output to remove.
    let archive = ArchiveFixtureBuilder::new()
        .file("pack/ok.txt", b"written first")
        .file("loose.txt", b"also written")
        .symlink("pack/link", "/etc/passwd")
        .build();

    let result = extract_archive(archive.as_slice(), &sandbox.dest);
    assert!(matches!(
        result,
        Err(ArchiveSafetyError::UnsupportedEntryType { .. })
    ));

    let left: Vec<_> = std::fs::read_dir(&sandbox.dest).unwrap().collect();
    assert!(left.is_empty(), "partial output remained: {left:?}");
    sandbox.assert_nothing_written_outside();
}

#[test]
fn failed_extraction_keeps_files_that_were_already_in_the_destination() {
    let sandbox = TestSandbox::new("cleanup-preexisting");
    std::fs::write(sandbox.dest.join("keep.txt"), "mine").unwrap();
    std::fs::create_dir_all(sandbox.dest.join("keep_dir")).unwrap();
    std::fs::write(sandbox.dest.join("keep_dir/inner.txt"), "mine too").unwrap();

    let archive = ArchiveFixtureBuilder::new()
        .file("new.txt", b"written first")
        .symlink("link", "/etc/passwd")
        .build();

    assert!(extract_archive(archive.as_slice(), &sandbox.dest).is_err());

    assert_eq!(
        std::fs::read_to_string(sandbox.dest.join("keep.txt")).unwrap(),
        "mine"
    );
    assert_eq!(
        std::fs::read_to_string(sandbox.dest.join("keep_dir/inner.txt")).unwrap(),
        "mine too"
    );
    assert!(!sandbox.dest.join("new.txt").exists());
}

#[test]
fn successful_extraction_is_not_cleaned_up() {
    let sandbox = TestSandbox::new("cleanup-success");
    let archive = ArchiveFixtureBuilder::new()
        .file("pack/ok.txt", b"kept")
        .build();

    extract_archive(archive.as_slice(), &sandbox.dest).unwrap();
    assert_eq!(
        std::fs::read_to_string(sandbox.dest.join("pack/ok.txt")).unwrap(),
        "kept"
    );
}
