//! Building a [`GitContext`] from a [`GitRepository`].

use canary_core::GitContext;

use crate::repository::GitRepository;

/// Collects Git metadata for a run, using `unavailable` (`None`) for any
/// field that could not be determined rather than failing the run.
pub fn collect_git_context(repo: &impl GitRepository) -> GitContext {
    GitContext {
        commit: repo.current_commit().unwrap_or(None),
        branch: repo.current_branch().unwrap_or(None),
        is_dirty: repo.is_dirty().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::GitError;

    /// Builds the `InvalidUtf8` error the tests use to simulate a failing
    /// `GitRepository` method.
    fn invalid_utf8_error() -> GitError {
        GitError::InvalidUtf8(String::from_utf8(vec![0xff, 0xfe]).unwrap_err())
    }

    struct FakeRepository {
        commit: Result<Option<String>, GitError>,
        branch: Result<Option<String>, GitError>,
        dirty: Result<bool, GitError>,
    }

    impl GitRepository for FakeRepository {
        fn current_commit(&self) -> Result<Option<String>, GitError> {
            match &self.commit {
                Ok(commit) => Ok(commit.clone()),
                Err(_) => Err(invalid_utf8_error()),
            }
        }
        fn current_branch(&self) -> Result<Option<String>, GitError> {
            match &self.branch {
                Ok(branch) => Ok(branch.clone()),
                Err(_) => Err(invalid_utf8_error()),
            }
        }
        fn is_dirty(&self) -> Result<bool, GitError> {
            match &self.dirty {
                Ok(dirty) => Ok(*dirty),
                Err(_) => Err(invalid_utf8_error()),
            }
        }
    }

    #[test]
    fn unavailable_fields_become_none_in_the_context() {
        let repo = FakeRepository {
            commit: Ok(None),
            branch: Ok(None),
            dirty: Ok(false),
        };
        let context = collect_git_context(&repo);
        assert_eq!(context.commit, None);
        assert_eq!(context.branch, None);
        assert_eq!(context.is_dirty, Some(false));
    }

    #[test]
    fn available_fields_are_carried_through() {
        let repo = FakeRepository {
            commit: Ok(Some("abc123".to_string())),
            branch: Ok(Some("main".to_string())),
            dirty: Ok(true),
        };
        let context = collect_git_context(&repo);
        assert_eq!(context.commit.as_deref(), Some("abc123"));
        assert_eq!(context.branch.as_deref(), Some("main"));
        assert_eq!(context.is_dirty, Some(true));
    }

    #[test]
    fn a_current_commit_error_degrades_to_a_none_commit_without_panicking() {
        let repo = FakeRepository {
            commit: Err(invalid_utf8_error()),
            branch: Ok(Some("main".to_string())),
            dirty: Ok(true),
        };
        let context = collect_git_context(&repo);
        assert_eq!(context.commit, None);
        assert_eq!(context.branch.as_deref(), Some("main"));
        assert_eq!(context.is_dirty, Some(true));
    }

    #[test]
    fn a_current_branch_error_degrades_to_a_none_branch_without_panicking() {
        let repo = FakeRepository {
            commit: Ok(Some("abc123".to_string())),
            branch: Err(invalid_utf8_error()),
            dirty: Ok(true),
        };
        let context = collect_git_context(&repo);
        assert_eq!(context.commit.as_deref(), Some("abc123"));
        assert_eq!(context.branch, None);
        assert_eq!(context.is_dirty, Some(true));
    }

    #[test]
    fn an_is_dirty_error_degrades_to_a_none_dirty_flag_without_panicking() {
        let repo = FakeRepository {
            commit: Ok(Some("abc123".to_string())),
            branch: Ok(Some("main".to_string())),
            dirty: Err(invalid_utf8_error()),
        };
        let context = collect_git_context(&repo);
        assert_eq!(context.commit.as_deref(), Some("abc123"));
        assert_eq!(context.branch.as_deref(), Some("main"));
        assert_eq!(context.is_dirty, None);
    }
}
