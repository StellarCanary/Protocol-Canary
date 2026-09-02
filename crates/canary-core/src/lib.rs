//! Compatibility domain model, planning, and policy engine for Stellar
//! Protocol Canary.
//!
//! This crate is the dependency root for the rest of the workspace: it
//! defines the shared types (protocol version, surface, status, results,
//! execution context) and the `CompatibilityTest` abstraction that every
//! surface runner implements, but it has no dependency on any other crate
//! in this workspace.

pub mod cache;
pub mod engine;
pub mod errors;
pub mod model;
pub mod planner;
pub mod policy;

pub use cache::{CacheKey, CacheStore};
pub use engine::{CompatibilityTest, ExecutionContext};
pub use errors::{CanaryError, ErrorCategory, ExitCode};
pub use model::{
    Capability, CompatibilityResult, FixtureMetadata, FixtureStore, GitContext, NetworkContext,
    NetworkName, ProjectContext, ProjectType, ProtocolPack, ProtocolVersion, RunOptions, Status,
    Surface,
};
pub use planner::CompatibilityPlanner;
pub use policy::{Policy, PolicyDecision, PolicyEvaluator};
