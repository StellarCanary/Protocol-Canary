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

    struct FakeRepository {
        commit: Option<String>,
        branch: Option<String>,
        dirty: bool,
    }

    impl GitRepository for FakeRepository {
        fn current_commit(&self) -> Result<Option<String>, GitError> {
            Ok(self.commit.clone())
        }
        fn current_branch(&self) -> Result<Option<String>, GitError> {
            Ok(self.branch.clone())
        }
        fn is_dirty(&self) -> Result<bool, GitError> {
            Ok(self.dirty)
        }
    }

    #[test]
    fn unavailable_fields_become_none_in_the_context() {
        let repo = FakeRepository {
            commit: None,
            branch: None,
            dirty: false,
        };
        let context = collect_git_context(&repo);
        assert_eq!(context.commit, None);
        assert_eq!(context.branch, None);
        assert_eq!(context.is_dirty, Some(false));
    }

    #[test]
    fn available_fields_are_carried_through() {
        let repo = FakeRepository {
            commit: Some("abc123".to_string()),
            branch: Some("main".to_string()),
            dirty: true,
        };
        let context = collect_git_context(&repo);
        assert_eq!(context.commit.as_deref(), Some("abc123"));
        assert_eq!(context.branch.as_deref(), Some("main"));
        assert_eq!(context.is_dirty, Some(true));
    }
}
