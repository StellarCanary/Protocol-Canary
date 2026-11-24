//! Git repository metadata collection for Stellar Protocol Canary.

pub mod discovery;
pub mod metadata;
pub mod repository;

pub use discovery::{resolve_head, GitDiscoveryError, HeadInfo};
pub use metadata::collect_git_context;
pub use repository::{CliGitRepository, GitError, GitRepository};
