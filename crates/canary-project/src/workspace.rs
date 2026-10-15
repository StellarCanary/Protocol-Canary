//! Cargo workspace member discovery (contract CF-07, unit C).
//!
//! A virtual workspace root has no dependencies of its own: the
//! `soroban-sdk` or `stellar-sdk` dependency lives in the member crates. This
//! module reads `[workspace] members` and `exclude` from the root
//! `Cargo.toml`, resolves the member paths, and reads each member's
//! `Cargo.toml` for its declared dependency names.
//!
//! The scan is pure: it runs no process, never calls `cargo`, never follows
//! a symbolic link and never leaves the project root. Every bound of CF-07
//! section 5 that is reached is reported as a [`Diagnostic`]; it never turns
//! into a silent "nothing found".

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::manifest::{parse_cargo_manifest, ProjectManifest};

/// Most workspace members that are scanned (decision D-06).
pub const MAX_WORKSPACE_MEMBERS: usize = 64;
/// Deepest member path, in segments below the root (CF-07 section 5).
pub const MAX_WORKSPACE_DEPTH: usize = 4;
/// Most directories visited while expanding member globs.
pub const MAX_DIRECTORIES_VISITED: usize = 2_000;
/// Most directory entries read while expanding member globs.
pub const MAX_ENTRIES_READ: usize = 10_000;
/// Largest manifest that is read (1 MiB).
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Directory names that are never scanned, anywhere (CF-07 section 5).
const EXCLUDED_DIR_NAMES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".gradle",
    ".cache",
];

/// What a [`Diagnostic`] is about. The spellings are CF-07 section 6 codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    ManifestUnparseable,
    ManifestTooLarge,
    ScanBoundReached,
    SymlinkSkipped,
    UnsupportedSyntax,
    WorkspaceMemberMissing,
    PathOutsideRoot,
}

impl DiagnosticCode {
    /// The kebab-case code used in evidence output.
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::ManifestUnparseable => "manifest-unparseable",
            DiagnosticCode::ManifestTooLarge => "manifest-too-large",
            DiagnosticCode::ScanBoundReached => "scan-bound-reached",
            DiagnosticCode::SymlinkSkipped => "symlink-skipped",
            DiagnosticCode::UnsupportedSyntax => "unsupported-syntax",
            DiagnosticCode::WorkspaceMemberMissing => "workspace-member-missing",
            DiagnosticCode::PathOutsideRoot => "path-outside-root",
        }
    }
}

/// Something the scan skipped or could not do, with a root-relative detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub detail: String,
}

/// One workspace member whose manifest was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMember {
    /// Root-relative directory, `/`-separated, no leading `./`.
    pub path: String,
    /// The dependency names the member's `Cargo.toml` declares.
    pub manifest: ProjectManifest,
}

/// The outcome of scanning a Cargo workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceScan {
    /// Members in UTF-8 byte order of their path, at most
    /// [`MAX_WORKSPACE_MEMBERS`].
    pub members: Vec<WorkspaceMember>,
    pub diagnostics: Vec<Diagnostic>,
}

impl WorkspaceScan {
    /// The union of every member's dependency names, sorted and unique.
    pub fn dependency_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .members
            .iter()
            .flat_map(|member| member.manifest.dependency_names.iter().cloned())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Whether any member declares the dependency `name`.
    pub fn has_dependency(&self, name: &str) -> bool {
        self.members
            .iter()
            .any(|member| member.manifest.has_dependency(name))
    }
}

/// Discovers the members of the Cargo workspace rooted at `root`.
///
/// Returns an empty scan, with no diagnostic, when `root` has no
/// `Cargo.toml` or its manifest has no `[workspace]` table with `members`.
pub fn scan_cargo_workspace(root: &Path) -> WorkspaceScan {
    let mut scan = WorkspaceScan::default();
    let manifest_path = root.join("Cargo.toml");
    let Ok(text) = read_limited(&manifest_path) else {
        return scan;
    };
    let Some(value) = text.and_then(|t| toml::from_str::<toml::Value>(&t).ok()) else {
        return scan;
    };
    let Some(workspace) = value.get("workspace") else {
        return scan;
    };
    let patterns = string_list(workspace.get("members"));
    let excludes = string_list(workspace.get("exclude"));
    if patterns.is_empty() {
        return scan;
    }

    let mut budget = Budget::default();
    let mut found: BTreeSet<String> = BTreeSet::new();
    for pattern in &patterns {
        expand_pattern(
            root,
            pattern,
            &mut found,
            &mut scan.diagnostics,
            &mut budget,
        );
    }

    let exclude_patterns: Vec<Vec<&str>> =
        excludes.iter().map(|p| p.split('/').collect()).collect();
    found.retain(|member| {
        let segments: Vec<&str> = member.split('/').collect();
        !exclude_patterns
            .iter()
            .any(|pattern| match_path(pattern, &segments))
    });

    let total = found.len();
    for member in found.into_iter().take(MAX_WORKSPACE_MEMBERS) {
        read_member(root, &member, &mut scan);
    }
    if total > MAX_WORKSPACE_MEMBERS {
        scan.diagnostics.push(Diagnostic {
            code: DiagnosticCode::ScanBoundReached,
            detail: format!(
                "workspace members limit {MAX_WORKSPACE_MEMBERS} ({total} found); the rest were not scanned"
            ),
        });
    }
    scan
}

#[derive(Default)]
struct Budget {
    directories: usize,
    entries: usize,
    reported: bool,
}

impl Budget {
    fn exhausted(&mut self, diagnostics: &mut Vec<Diagnostic>) -> bool {
        let over = self.directories > MAX_DIRECTORIES_VISITED || self.entries > MAX_ENTRIES_READ;
        if over && !self.reported {
            self.reported = true;
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::ScanBoundReached,
                detail: format!(
                    "directories visited limit {MAX_DIRECTORIES_VISITED} or entries read limit {MAX_ENTRIES_READ}"
                ),
            });
        }
        over
    }
}

fn string_list(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn has_wildcard(segment: &str) -> bool {
    segment.contains('*') || segment.contains('?')
}

/// Matches one path segment against a pattern of literals, `*` and `?`.
fn match_segment(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) && pattern[p] != '*' {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            mark = t;
            p += 1;
        } else if let Some(s) = star {
            p = s + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

/// Matches a whole root-relative path against an `exclude` pattern.
fn match_path(pattern: &[&str], path: &[&str]) -> bool {
    pattern.len() == path.len()
        && pattern
            .iter()
            .zip(path)
            .all(|(pattern, segment)| match_segment(pattern, segment))
}

fn push(diagnostics: &mut Vec<Diagnostic>, code: DiagnosticCode, detail: impl Into<String>) {
    diagnostics.push(Diagnostic {
        code,
        detail: detail.into(),
    });
}

/// Expands one `members` entry into root-relative directories.
fn expand_pattern(
    root: &Path,
    pattern: &str,
    found: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
    budget: &mut Budget,
) {
    if pattern.is_empty()
        || pattern.starts_with('/')
        || pattern.contains('\\')
        || pattern.split('/').any(|s| s == "..")
        || pattern.as_bytes().get(1) == Some(&b':')
    {
        push(
            diagnostics,
            DiagnosticCode::PathOutsideRoot,
            format!("workspace member pattern {pattern:?} is not a relative path inside the root"),
        );
        return;
    }
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    if pattern == "." || pattern.is_empty() {
        return; // The root itself is read separately.
    }
    if pattern.contains('[') || pattern.contains('{') || pattern.contains("**") {
        push(
            diagnostics,
            DiagnosticCode::UnsupportedSyntax,
            format!("workspace member pattern {pattern:?} uses glob syntax other than * and ?"),
        );
        return;
    }
    let segments: Vec<&str> = pattern.trim_end_matches('/').split('/').collect();
    if segments.iter().any(|s| s.is_empty() || *s == ".") {
        push(
            diagnostics,
            DiagnosticCode::UnsupportedSyntax,
            format!("workspace member pattern {pattern:?} has an empty or '.' segment"),
        );
        return;
    }
    if segments.len() > MAX_WORKSPACE_DEPTH {
        push(
            diagnostics,
            DiagnosticCode::ScanBoundReached,
            format!(
                "workspace member pattern {pattern:?} is deeper than {MAX_WORKSPACE_DEPTH} levels"
            ),
        );
        return;
    }
    let before = found.len();
    expand_segments(root, "", &segments, found, diagnostics, budget);
    let literal = !segments.iter().any(|s| has_wildcard(s));
    if literal && found.len() == before && !budget.reported {
        // Reported only when nothing else (a skipped link) explains it.
        let already = diagnostics.iter().any(|d| d.detail.contains(pattern));
        if !already {
            push(
                diagnostics,
                DiagnosticCode::WorkspaceMemberMissing,
                format!("workspace member {pattern:?} is not a directory"),
            );
        }
    }
}

fn expand_segments(
    root: &Path,
    base: &str,
    segments: &[&str],
    found: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
    budget: &mut Budget,
) {
    let Some((segment, rest)) = segments.split_first() else {
        found.insert(base.to_string());
        return;
    };
    if EXCLUDED_DIR_NAMES.contains(segment) {
        return;
    }
    let join = |name: &str| {
        if base.is_empty() {
            name.to_string()
        } else {
            format!("{base}/{name}")
        }
    };
    if !has_wildcard(segment) {
        let relative = join(segment);
        match directory_kind(root, &relative) {
            DirKind::Directory => {
                expand_segments(root, &relative, rest, found, diagnostics, budget)
            }
            DirKind::Link => push(
                diagnostics,
                DiagnosticCode::SymlinkSkipped,
                format!("{relative} is a symbolic link and was not followed"),
            ),
            DirKind::Other => {}
        }
        return;
    }

    budget.directories += 1;
    if budget.exhausted(diagnostics) {
        return;
    }
    let directory = to_path(root, base);
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return;
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        budget.entries += 1;
        if budget.exhausted(diagnostics) {
            return;
        }
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names.sort();
    for name in names {
        if EXCLUDED_DIR_NAMES.contains(&name.as_str()) || !match_segment(segment, &name) {
            continue;
        }
        let relative = join(&name);
        match directory_kind(root, &relative) {
            DirKind::Directory => {
                expand_segments(root, &relative, rest, found, diagnostics, budget)
            }
            DirKind::Link => push(
                diagnostics,
                DiagnosticCode::SymlinkSkipped,
                format!("{relative} is a symbolic link and was not followed"),
            ),
            DirKind::Other => {}
        }
    }
}

enum DirKind {
    Directory,
    Link,
    Other,
}

fn to_path(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .filter(|s| !s.is_empty())
        .fold(root.to_path_buf(), |path, segment| path.join(segment))
}

fn directory_kind(root: &Path, relative: &str) -> DirKind {
    match std::fs::symlink_metadata(to_path(root, relative)) {
        Ok(meta) if meta.file_type().is_symlink() => DirKind::Link,
        Ok(meta) if meta.is_dir() => DirKind::Directory,
        _ => DirKind::Other,
    }
}

/// Reads a file of at most [`MAX_MANIFEST_BYTES`]. `Ok(None)` means too large.
fn read_limited(path: &Path) -> Result<Option<String>, std::io::Error> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
    }
    if meta.len() > MAX_MANIFEST_BYTES {
        return Ok(None);
    }
    std::fs::read_to_string(path).map(Some)
}

fn read_member(root: &Path, member: &str, scan: &mut WorkspaceScan) {
    let manifest_relative = format!("{member}/Cargo.toml");
    let manifest_path = to_path(root, &manifest_relative);
    match read_limited(&manifest_path) {
        Err(_) => push(
            &mut scan.diagnostics,
            DiagnosticCode::WorkspaceMemberMissing,
            format!("workspace member {member:?} has no readable Cargo.toml"),
        ),
        Ok(None) => push(
            &mut scan.diagnostics,
            DiagnosticCode::ManifestTooLarge,
            format!("{manifest_relative} is larger than {MAX_MANIFEST_BYTES} bytes"),
        ),
        Ok(Some(text)) => match parse_cargo_manifest(&text) {
            Some(manifest) => scan.members.push(WorkspaceMember {
                path: member.to_string(),
                manifest,
            }),
            None => push(
                &mut scan.diagnostics,
                DiagnosticCode::ManifestUnparseable,
                format!("{manifest_relative} is not valid TOML"),
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_globs_support_star_and_question_mark_only() {
        assert!(match_segment("*", "anything"));
        assert!(match_segment("c*", "contracts"));
        assert!(match_segment("*s", "contracts"));
        assert!(match_segment("a?c", "abc"));
        assert!(!match_segment("a?c", "ac"));
        assert!(!match_segment("a*c", "abd"));
        assert!(match_segment("a*b*c", "aXXbYYc"));
        assert!(match_segment("", ""));
        assert!(!match_segment("", "a"));
    }

    #[test]
    fn a_star_never_crosses_a_path_separator() {
        assert!(match_path(&["crates", "*"], &["crates", "a"]));
        assert!(!match_path(&["crates", "*"], &["crates", "a", "b"]));
        assert!(!match_path(&["*"], &["crates", "a"]));
    }
}
