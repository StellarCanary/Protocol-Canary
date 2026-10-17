//! Git repository and worktree HEAD discovery and resolution.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Errors that can occur when discovering or resolving a Git repository or worktree.
#[derive(Debug, thiserror::Error)]
pub enum GitDiscoveryError {
    /// No `.git` file or directory was found at the given location.
    #[error("git repository not found at '{0}'")]
    NotFound(PathBuf),

    /// The `.git` file is corrupted or not a valid worktree pointer.
    #[error("corrupted .git worktree pointer file at '{path}': {reason}")]
    CorruptPointerFile { path: PathBuf, reason: String },

    /// The `.git` pointer file references a `gitdir` path that does not exist.
    #[error("dangling .git worktree pointer at '{pointer}' pointing to nonexistent '{target}'")]
    DanglingPointer { pointer: PathBuf, target: PathBuf },

    /// The HEAD file or the referenced branch could not be resolved.
    #[error("invalid or unresolvable HEAD reference: {0}")]
    InvalidReference(String),

    /// An I/O error occurred during discovery.
    #[error("I/O error during git discovery: {0}")]
    Io(#[from] std::io::Error),
}

/// Information about a resolved Git `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadInfo {
    /// The full 40-character (or 64-character) commit SHA.
    pub commit: String,
    /// The branch name, or `None` if HEAD is detached.
    pub branch: Option<String>,
}

impl HeadInfo {
    /// Creates a new [`HeadInfo`].
    pub fn new(commit: impl Into<String>, branch: Option<impl Into<String>>) -> Self {
        Self {
            commit: commit.into(),
            branch: branch.map(Into::into),
        }
    }

    /// The commit hash.
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// The branch name, or `None` if detached.
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    /// Returns `true` if HEAD is detached.
    pub fn is_detached(&self) -> bool {
        self.branch.is_none()
    }
}

impl From<HeadInfo> for canary_core::GitContext {
    fn from(info: HeadInfo) -> Self {
        canary_core::GitContext {
            commit: Some(info.commit),
            branch: info.branch,
            is_dirty: None,
        }
    }
}

fn is_valid_commit_sha(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn resolve_ref_sha(gitdir: &Path, common_dir: &Path, ref_path: &str) -> Option<String> {
    // 1. Loose ref in per-worktree gitdir
    let loose_in_gitdir = gitdir.join(ref_path);
    if let Ok(content) = fs::read_to_string(&loose_in_gitdir) {
        let trimmed = content.trim();
        if is_valid_commit_sha(trimmed) {
            return Some(trimmed.to_string());
        }
    }

    // 2. Loose ref in common_dir
    let loose_in_common = common_dir.join(ref_path);
    if let Ok(content) = fs::read_to_string(&loose_in_common) {
        let trimmed = content.trim();
        if is_valid_commit_sha(trimmed) {
            return Some(trimmed.to_string());
        }
    }

    // 3. Packed-refs in common_dir
    let packed_refs = common_dir.join("packed-refs");
    if let Ok(content) = fs::read_to_string(&packed_refs) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with('^') || line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            if let (Some(sha), Some(name)) = (parts.next(), parts.next()) {
                if name == ref_path && is_valid_commit_sha(sha) {
                    return Some(sha.to_string());
                }
            }
        }
    }

    None
}

fn run_git_rev_parse(worktree_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(worktree_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if is_valid_commit_sha(trimmed) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Resolves the HEAD commit hash and branch name for a repository or linked worktree.
///
/// # Worktree Support
///
/// If `path` contains a `.git` file (a linked Git worktree), this function
/// parses the `gitdir: <path>` pointer, handles relative or absolute paths,
/// validates that the target gitdir exists, and resolves the worktree's HEAD.
///
/// If the `.git` pointer file is malformed or points to a nonexistent directory,
/// returns [`GitDiscoveryError`] without panicking.
pub fn resolve_head(path: impl AsRef<Path>) -> Result<HeadInfo, GitDiscoveryError> {
    let path_ref = path.as_ref();
    let (worktree_root, git_path) =
        if path_ref.is_file() || path_ref.file_name() == Some(std::ffi::OsStr::new(".git")) {
            (
                path_ref.parent().unwrap_or(path_ref).to_path_buf(),
                path_ref.to_path_buf(),
            )
        } else {
            (path_ref.to_path_buf(), path_ref.join(".git"))
        };

    if !git_path.exists() {
        return Err(GitDiscoveryError::NotFound(git_path));
    }

    let (resolved_gitdir, common_dir) = if git_path.is_file() {
        let bytes = fs::read(&git_path)?;
        let content =
            String::from_utf8(bytes).map_err(|_| GitDiscoveryError::CorruptPointerFile {
                path: git_path.clone(),
                reason: "pointer file is not valid UTF-8".to_string(),
            })?;

        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(GitDiscoveryError::CorruptPointerFile {
                path: git_path.clone(),
                reason: "pointer file is empty".to_string(),
            });
        }

        let raw_target = if let Some(rest) = trimmed.strip_prefix("gitdir:") {
            rest.trim()
        } else {
            return Err(GitDiscoveryError::CorruptPointerFile {
                path: git_path.clone(),
                reason: "missing 'gitdir:' prefix".to_string(),
            });
        };

        if raw_target.is_empty() {
            return Err(GitDiscoveryError::CorruptPointerFile {
                path: git_path.clone(),
                reason: "empty gitdir path in pointer file".to_string(),
            });
        }

        let target_path = PathBuf::from(raw_target);
        let resolved = if target_path.is_relative()
            && !raw_target.starts_with('/')
            && !raw_target.starts_with('\\')
        {
            worktree_root.join(&target_path)
        } else {
            target_path
        };

        if !resolved.is_dir() {
            return Err(GitDiscoveryError::DanglingPointer {
                pointer: git_path.clone(),
                target: resolved,
            });
        }

        let commondir_file = resolved.join("commondir");
        let common = if commondir_file.is_file() {
            if let Ok(cd_content) = fs::read_to_string(&commondir_file) {
                let cd_trimmed = cd_content.trim();
                let cd_path = PathBuf::from(cd_trimmed);
                if cd_path.is_relative()
                    && !cd_trimmed.starts_with('/')
                    && !cd_trimmed.starts_with('\\')
                {
                    resolved.join(&cd_path)
                } else {
                    cd_path
                }
            } else {
                resolved.clone()
            }
        } else {
            resolved.clone()
        };

        (resolved, common)
    } else if git_path.is_dir() {
        (git_path.clone(), git_path)
    } else {
        return Err(GitDiscoveryError::NotFound(git_path));
    };

    let head_file = resolved_gitdir.join("HEAD");
    if !head_file.is_file() {
        return Err(GitDiscoveryError::InvalidReference(format!(
            "HEAD file not found at '{}'",
            head_file.display()
        )));
    }

    let head_content = fs::read_to_string(&head_file)?;
    let head_trimmed = head_content.trim();

    if let Some(ref_target) = head_trimmed.strip_prefix("ref:") {
        let ref_path = ref_target.trim();
        let branch_name = if let Some(name) = ref_path.strip_prefix("refs/heads/") {
            name.to_string()
        } else {
            ref_path.to_string()
        };

        let commit_sha = resolve_ref_sha(&resolved_gitdir, &common_dir, ref_path)
            .or_else(|| run_git_rev_parse(&worktree_root));

        match commit_sha {
            Some(sha) => Ok(HeadInfo {
                commit: sha,
                branch: Some(branch_name),
            }),
            None => Err(GitDiscoveryError::InvalidReference(format!(
                "could not resolve commit for ref '{ref_path}'"
            ))),
        }
    } else if is_valid_commit_sha(head_trimmed) {
        Ok(HeadInfo {
            commit: head_trimmed.to_string(),
            branch: None,
        })
    } else {
        Err(GitDiscoveryError::InvalidReference(format!(
            "malformed HEAD contents: '{head_trimmed}'"
        )))
    }
}
