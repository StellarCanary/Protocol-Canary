//! Compatibility domain model and policy engine for Stellar Protocol
//! Canary.
//!
//! This crate is the dependency root for the rest of the workspace: it
//! defines the shared types (protocol version, surface, status, results,
//! execution context) that every surface runner works with, but it has no
//! dependency on any other crate in this workspace.

pub mod cache;
pub mod engine;
pub mod errors;
pub mod model;
pub mod policy;

pub use cache::{CacheKey, CacheStore};
pub use engine::ExecutionContext;
pub use errors::{CanaryError, ErrorCategory, ExitCode};
pub use model::{
    Capability, CompatibilityResult, FixtureMetadata, FixtureStore, GitContext, NetworkContext,
    NetworkName, ProjectContext, ProjectType, ProtocolPack, ProtocolVersion, RunOptions, Status,
    Surface,
};
pub use policy::{
    exit_code_for_run, DefaultPolicyEvaluator, Policy, PolicyDecision, PolicyEvaluator,
};
