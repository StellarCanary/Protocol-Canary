//! Integration tests for Git commit SHA and branch resolution in linked worktrees.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use canary_git::{resolve_head, CliGitRepository, GitDiscoveryError, GitRepository};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Scratch directory cleaned up on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "canary-git-test-{prefix}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("failed to create scratch directory");
        Scratch(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Helper executing `git` commands configured with test author and committer metadata.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Canary Test")
        .env("GIT_AUTHOR_EMAIL", "canary-test@example.com")
        .env("GIT_COMMITTER_NAME", "Canary Test")
        .env("GIT_COMMITTER_EMAIL", "canary-test@example.com")
        .output()
        .expect("git must be installed to run worktree integration tests");

    assert!(
        output.status.success(),
        "git command failed: git {:?} in {}\nstdout: {}\nstderr: {}",
        args,
        dir.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// Harness initializing a test repository with an initial commit.
struct TestRepo {
    _scratch: Scratch,
    repo_path: PathBuf,
    initial_commit: String,
}

impl TestRepo {
    fn new(name: &str) -> Self {
        let scratch = Scratch::new(name);
        let repo_path = scratch.path().to_path_buf();

        run_git(&repo_path, &["init", "--quiet", "--initial-branch=main"]);
        std::fs::write(repo_path.join("README.md"), "# Canary Test Repo\n").unwrap();
        run_git(&repo_path, &["add", "README.md"]);
        run_git(&repo_path, &["commit", "--quiet", "-m", "initial commit"]);
        let commit = run_git(&repo_path, &["rev-parse", "HEAD"]);

        TestRepo {
            _scratch: scratch,
            repo_path,
            initial_commit: commit,
        }
    }

    /// Creates a linked worktree for `branch_name` in a separate scratch directory.
    fn create_worktree(&self, name: &str, branch_name: &str) -> (Scratch, PathBuf) {
        let wt_scratch = Scratch::new(name);
        let wt_path = wt_scratch.path().to_path_buf();
        // Remove empty dir so git worktree add can initialize it cleanly
        let _ = std::fs::remove_dir(&wt_path);

        run_git(
            &self.repo_path,
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                wt_path.to_str().unwrap(),
                "main",
            ],
        );

        (wt_scratch, wt_path)
    }
}

#[test]
fn worktree_resolves_head_commit_and_active_branch() {
    let repo = TestRepo::new("wt-active");
    let (_wt_scratch, wt_path) = repo.create_worktree("wt-branch", "feature-worktree");

    // Verify .git in a worktree is a file pointing to gitdir:
    let dot_git = wt_path.join(".git");
    assert!(
        dot_git.is_file(),
        ".git must be a file in a linked worktree"
    );
    let pointer_content = std::fs::read_to_string(&dot_git).unwrap();
    assert!(
        pointer_content.starts_with("gitdir:"),
        ".git pointer must start with gitdir:"
    );

    // Test resolve_head inside worktree directory
    let head = resolve_head(&wt_path).expect("resolve_head should resolve worktree HEAD");
    assert_eq!(head.commit(), repo.initial_commit);
    assert_eq!(head.branch(), Some("feature-worktree"));
    assert!(!head.is_detached());

    // Also verify CliGitRepository reads the same commit and branch inside the worktree
    let cli_repo = CliGitRepository::new(&wt_path);
    assert_eq!(
        cli_repo.current_commit().unwrap().as_deref(),
        Some(repo.initial_commit.as_str())
    );
    assert_eq!(
        cli_repo.current_branch().unwrap().as_deref(),
        Some("feature-worktree")
    );
    assert!(!cli_repo.is_dirty().unwrap());

    // Make a new commit in the worktree and verify resolution updates
    std::fs::write(wt_path.join("feature.txt"), "worktree change\n").unwrap();
    assert!(cli_repo.is_dirty().unwrap());
    run_git(&wt_path, &["add", "feature.txt"]);
    run_git(&wt_path, &["commit", "--quiet", "-m", "worktree commit"]);

    let new_commit = run_git(&wt_path, &["rev-parse", "HEAD"]);
    let updated_head = resolve_head(&wt_path).expect("resolve_head should succeed after commit");
    assert_eq!(updated_head.commit(), new_commit);
    assert_ne!(updated_head.commit(), repo.initial_commit);
    assert_eq!(updated_head.branch(), Some("feature-worktree"));
    assert!(!updated_head.is_detached());

    assert_eq!(
        cli_repo.current_commit().unwrap().as_deref(),
        Some(new_commit.as_str())
    );
    assert_eq!(
        cli_repo.current_branch().unwrap().as_deref(),
        Some("feature-worktree")
    );
    assert!(!cli_repo.is_dirty().unwrap());
}

/// Renders `to` relative to the directory `from`, both absolute, using `..`
/// segments for the parent hops needed to reach a common ancestor.
fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let common = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut relative = PathBuf::new();
    for component in &from_components[common..] {
        assert!(
            matches!(component, std::path::Component::Normal(_)),
            "unexpected component in scratch path: {component:?}"
        );
        relative.push("..");
    }
    for component in &to_components[common..] {
        relative.push(component.as_os_str());
    }
    relative
}

/// A worktree can be relocated with a `.git` pointer that uses a relative
/// `gitdir:` path (e.g. `gitdir: ../../.git/worktrees/wt`). Resolution must
/// work from that relative pointer exactly as it does from the absolute one
/// Git writes by default.
#[test]
fn worktree_resolves_relative_gitdir_pointer() {
    let repo = TestRepo::new("wt-relative");
    let (_wt_scratch, wt_path) = repo.create_worktree("wt-relative-branch", "feature-relative");

    // Read the absolute target Git wrote, then rewrite the pointer relative
    // to the worktree root, as a relocated worktree would carry it.
    let dot_git = wt_path.join(".git");
    let pointer_content = std::fs::read_to_string(&dot_git).unwrap();
    let absolute_target = pointer_content
        .strip_prefix("gitdir:")
        .expect("worktree pointer starts with gitdir:")
        .trim();
    let absolute_target = PathBuf::from(absolute_target);
    assert!(
        absolute_target.is_absolute(),
        "git writes an absolute gitdir"
    );

    let relative_target = relative_path(&wt_path, &absolute_target);
    assert!(
        relative_target.is_relative(),
        "the rewritten pointer must be relative"
    );
    std::fs::write(&dot_git, format!("gitdir: {}\n", relative_target.display())).unwrap();

    // resolve_head follows the relative pointer to the same commit and branch.
    let head =
        resolve_head(&wt_path).expect("resolve_head should follow a relative gitdir pointer");
    assert_eq!(head.commit(), repo.initial_commit);
    assert_eq!(head.branch(), Some("feature-relative"));
    assert!(!head.is_detached());

    // The CLI-backed repository resolves identically from the relative pointer.
    let cli_repo = CliGitRepository::new(&wt_path);
    assert_eq!(
        cli_repo.current_commit().unwrap().as_deref(),
        Some(repo.initial_commit.as_str())
    );
    assert_eq!(
        cli_repo.current_branch().unwrap().as_deref(),
        Some("feature-relative")
    );
    assert!(!cli_repo.is_dirty().unwrap());
}

#[test]
fn worktree_resolves_detached_head() {
    let repo = TestRepo::new("wt-detach");
    let (_wt_scratch, wt_path) = repo.create_worktree("wt-detached", "branch-to-detach");

    // Detach HEAD in the linked worktree
    run_git(
        &wt_path,
        &["checkout", "--quiet", "--detach", &repo.initial_commit],
    );

    let head = resolve_head(&wt_path).expect("resolve_head should succeed on detached worktree");
    assert_eq!(head.commit(), repo.initial_commit);
    assert_eq!(head.branch(), None);
    assert!(head.is_detached());

    // Check CliGitRepository in detached worktree
    let cli_repo = CliGitRepository::new(&wt_path);
    assert_eq!(
        cli_repo.current_commit().unwrap().as_deref(),
        Some(repo.initial_commit.as_str())
    );
    assert_eq!(cli_repo.current_branch().unwrap(), None);
}

#[test]
fn negative_test_corrupted_gitdir_pointer_file_returns_error_without_panic() {
    let scratch = Scratch::new("wt-corrupt");
    let dot_git = scratch.path().join(".git");

    // Case 1: Arbitrary corrupt text without gitdir:
    std::fs::write(&dot_git, "not a valid git pointer file\n").unwrap();
    let err = resolve_head(scratch.path()).expect_err("must return error for corrupted pointer");
    assert!(
        matches!(err, GitDiscoveryError::CorruptPointerFile { .. }),
        "expected CorruptPointerFile, got {err:?}"
    );

    // Case 2: Empty pointer file
    std::fs::write(&dot_git, "").unwrap();
    let err = resolve_head(scratch.path()).expect_err("must return error for empty pointer");
    assert!(
        matches!(err, GitDiscoveryError::CorruptPointerFile { .. }),
        "expected CorruptPointerFile, got {err:?}"
    );

    // Case 3: gitdir prefix with empty path
    std::fs::write(&dot_git, "gitdir:    \n").unwrap();
    let err = resolve_head(scratch.path()).expect_err("must return error for empty gitdir path");
    assert!(
        matches!(err, GitDiscoveryError::CorruptPointerFile { .. }),
        "expected CorruptPointerFile, got {err:?}"
    );
}

#[test]
fn negative_test_dangling_gitdir_pointer_file_returns_error_without_panic() {
    let scratch = Scratch::new("wt-dangling");
    let dot_git = scratch.path().join(".git");

    // Pointer pointing to nonexistent directory
    let nonexistent_target = scratch
        .path()
        .join("missing-main-repo")
        .join(".git")
        .join("worktrees")
        .join("wt-nonexistent");
    std::fs::write(
        &dot_git,
        format!("gitdir: {}\n", nonexistent_target.display()),
    )
    .unwrap();

    let err = resolve_head(scratch.path()).expect_err("must return error for dangling pointer");
    assert!(
        matches!(err, GitDiscoveryError::DanglingPointer { .. }),
        "expected DanglingPointer, got {err:?}"
    );
}

#[test]
fn standard_repository_resolves_accurately() {
    let repo = TestRepo::new("standard-repo");

    let head = resolve_head(&repo.repo_path).expect("resolve_head should succeed on standard repo");
    assert_eq!(head.commit(), repo.initial_commit);
    assert_eq!(head.branch(), Some("main"));
    assert!(!head.is_detached());
}
