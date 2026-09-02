//! Git repository metadata collection for Stellar Protocol Canary.

pub mod metadata;
pub mod repository;

pub use metadata::collect_git_context;
pub use repository::{CliGitRepository, GitError, GitRepository};
