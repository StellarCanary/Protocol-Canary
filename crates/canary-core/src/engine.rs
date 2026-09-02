//! The execution context and compatibility-test abstraction that every
//! surface runner implements against.

use crate::cache::CacheStore;
use crate::errors::CanaryError;
use crate::model::{
    CompatibilityResult, FixtureStore, GitContext, NetworkContext, ProjectContext, ProtocolVersion,
    RunOptions, Surface,
};

/// Everything a [`CompatibilityTest`] needs to run, explicitly. There is no
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

/// A single compatibility assertion for one protocol version and surface.
pub trait CompatibilityTest {
    fn id(&self) -> &str;

    fn protocol_version(&self) -> ProtocolVersion;

    fn surface(&self) -> Surface;

    fn execute(&self, context: &ExecutionContext) -> Result<CompatibilityResult, CanaryError>;
}
