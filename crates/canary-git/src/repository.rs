//! Reading Git repository metadata by shelling out to the `git` binary.
//!
//! This intentionally does not depend on `git2`/libgit2: everything this
//! project needs (current commit, current branch, dirty status) is a
//! one-line `git` invocation, and avoiding a linked C library keeps the
//! build simple and portable. `git` itself is a reasonable thing to
//! assume is on `PATH` for a developer tool used inside a Git checkout.
//!
//! Per the project's rule that normal, non-Git usage must never fail a
//! run: a missing `git` binary, a directory that is not a repository, and
//! any other non-zero exit from `git` all resolve to `Ok(None)` /
//! `Ok(false)`, never an error. [`GitError`] is reserved for a git
//! invocation that ran and exited successfully but produced output this
//! crate cannot interpret (not valid UTF-8).

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git produced output that was not valid UTF-8: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
}

/// Git repository metadata a compatibility run can record.
pub trait GitRepository {
    fn current_commit(&self) -> Result<Option<String>, GitError>;
    fn current_branch(&self) -> Result<Option<String>, GitError>;
    fn is_dirty(&self) -> Result<bool, GitError>;
}

/// A [`GitRepository`] backed by the `git` CLI.
pub struct CliGitRepository {
    root: PathBuf,
}

impl CliGitRepository {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        CliGitRepository { root: root.into() }
    }

    fn run(&self, args: &[&str]) -> Result<Option<String>, GitError> {
        let output = match Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
        {
            Ok(output) => output,
            // `git` is not installed, or could not be spawned for some
            // other OS-level reason: treat exactly like "not a repository".
            Err(_) => return Ok(None),
        };
        if !output.status.success() {
            return Ok(None);
        }
        let text = String::from_utf8(output.stdout)?;
        let trimmed = text.trim();
        Ok(if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        })
    }
}

impl GitRepository for CliGitRepository {
    fn current_commit(&self) -> Result<Option<String>, GitError> {
        self.run(&["rev-parse", "HEAD"])
    }

    fn current_branch(&self) -> Result<Option<String>, GitError> {
        match self.run(&["symbolic-ref", "--short", "-q", "HEAD"])? {
            Some(branch) => Ok(Some(branch)),
            // Detached HEAD: symbolic-ref fails, but the repo is real.
            None => Ok(None),
        }
    }

    fn is_dirty(&self) -> Result<bool, GitError> {
        Ok(self
            .run(&["status", "--porcelain"])?
            .is_some_and(|s| !s.is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        path.push(unique);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn run(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Canary Test")
            .env("GIT_AUTHOR_EMAIL", "canary-test@example.com")
            .env("GIT_COMMITTER_NAME", "Canary Test")
            .env("GIT_COMMITTER_EMAIL", "canary-test@example.com")
            .status()
            .expect("git must be installed to run this test");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn a_non_repository_directory_returns_unavailable_rather_than_erroring() {
        let dir = temp_dir("canary-git-non-repo");
        let repo = CliGitRepository::new(&dir);
        assert_eq!(repo.current_commit().unwrap(), None);
        assert_eq!(repo.current_branch().unwrap(), None);
        assert!(!repo.is_dirty().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clean_repository_reports_commit_and_branch_and_is_not_dirty() {
        let dir = temp_dir("canary-git-clean-repo");
        run(&dir, &["init", "--quiet", "--initial-branch=main"]);
        std::fs::write(dir.join("file.txt"), "content").unwrap();
        run(&dir, &["add", "file.txt"]);
        run(&dir, &["commit", "--quiet", "-m", "initial commit"]);

        let repo = CliGitRepository::new(&dir);
        let commit = repo.current_commit().unwrap();
        assert!(commit.is_some());
        assert_eq!(commit.unwrap().len(), 40);
        assert_eq!(repo.current_branch().unwrap().as_deref(), Some("main"));
        assert!(!repo.is_dirty().unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_uncommitted_change_is_reported_as_dirty() {
        let dir = temp_dir("canary-git-dirty-repo");
        run(&dir, &["init", "--quiet", "--initial-branch=main"]);
        std::fs::write(dir.join("file.txt"), "content").unwrap();
        run(&dir, &["add", "file.txt"]);
        run(&dir, &["commit", "--quiet", "-m", "initial commit"]);
        std::fs::write(dir.join("file.txt"), "changed").unwrap();

        let repo = CliGitRepository::new(&dir);
        assert!(repo.is_dirty().unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
