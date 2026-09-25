//! The execution context that every surface runner is handed.

use crate::cache::CacheStore;
use crate::model::{
    FixtureStore, GitContext, NetworkContext, ProjectContext, ProtocolVersion, RunOptions,
};

/// Everything a compatibility test needs to run, explicitly. There is no
/// hidden global state: two runs constructed with equal `ExecutionContext`
/// values (modulo genuinely live network state) must behave identically.
pub struct ExecutionContext {
    pub protocol: ProtocolVersion,
    pub project: ProjectContext,
    pub network: NetworkContext,
    pub fixtures: FixtureStore,
    pub git: GitContext,
    pub cache: CacheStore,
    pub options: RunOptions,
}
