//! Terminal, JSON, and Markdown reporters for Stellar Protocol Canary.
//!
//! Every reporter consumes the same normalized [`ReportInput`] — none of
//! them re-run anything or compute their own pass/fail verdict; that is
//! already decided by `canary_core::PolicyEvaluator` before a report is
//! ever rendered.

pub mod json;
pub mod markdown;
pub mod terminal;

use canary_core::{
    CompatibilityResult, GitContext, NetworkName, PolicyDecision, ProjectType, ProtocolVersion,
    Surface,
};

pub use json::JsonReporter;
pub use markdown::MarkdownReporter;
pub use terminal::TerminalReporter;

/// A fixture the planner decided not to run, and why — carried into the
/// report as plain data so reporters don't need to depend on
/// `canary-runner`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipSummary {
    pub fixture_id: String,
    pub surface: Surface,
    pub reason: String,
}

/// What is known about the project under test, for the report header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSummary {
    pub name: String,
    pub project_type: ProjectType,
}

/// What is known about the live network, when a live check ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSummary {
    pub name: NetworkName,
    pub observed_protocol: Option<ProtocolVersion>,
}

/// Everything a reporter needs to render a finished run. This is the only
/// input any reporter takes.
#[derive(Debug, Clone)]
pub struct ReportInput {
    pub tool_version: String,
    pub target_protocol: ProtocolVersion,
    pub project: ProjectSummary,
    pub network: Option<NetworkSummary>,
    pub results: Vec<CompatibilityResult>,
    pub skipped: Vec<SkipSummary>,
    pub decision: PolicyDecision,
    pub git: GitContext,
    pub verbose: bool,
}

impl ReportInput {
    pub fn results_for(&self, surface: Surface) -> impl Iterator<Item = &CompatibilityResult> {
        self.results.iter().filter(move |r| r.surface == surface)
    }

    pub fn has_any_error(&self) -> bool {
        self.results
            .iter()
            .any(|r| r.status == canary_core::Status::Error)
    }
}

/// The fixed surface display order used by every reporter, matching the
/// order fixtures are executed in (see `canary-runner`'s execution
/// engine).
pub const SURFACE_ORDER: [Surface; 3] = [Surface::Xdr, Surface::Rpc, Surface::Soroban];
