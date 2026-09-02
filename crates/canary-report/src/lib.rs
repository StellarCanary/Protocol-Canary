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

    /// The overall outcome every reporter renders, factoring in that an
    /// execution error overrides the underlying policy decision (see
    /// `canary_core::exit_code_for_run`, which applies the same rule to
    /// the process exit code).
    pub fn overall_status(&self) -> ReportStatus {
        if self.has_any_error() {
            ReportStatus::Error
        } else {
            match self.decision {
                PolicyDecision::Pass => ReportStatus::Pass,
                PolicyDecision::Warning => ReportStatus::Warning,
                PolicyDecision::Fail => ReportStatus::Fail,
            }
        }
    }
}

/// The overall outcome of a run, as every reporter renders it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStatus {
    Pass,
    Warning,
    Fail,
    Error,
}

impl ReportStatus {
    /// The lowercase machine-readable form used in JSON output.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportStatus::Pass => "pass",
            ReportStatus::Warning => "warning",
            ReportStatus::Fail => "fail",
            ReportStatus::Error => "error",
        }
    }
}

/// The fixed surface display order used by every reporter, matching the
/// order fixtures are executed in (see `canary-runner`'s execution
/// engine).
pub const SURFACE_ORDER: [Surface; 3] = [Surface::Xdr, Surface::Rpc, Surface::Soroban];

/// The heading each reporter uses for a surface. `Surface`'s own
/// `Display` is lowercase (it doubles as the JSON `"surface"` value), but
/// report headings read better capitalized as an acronym/proper noun.
pub fn surface_heading(surface: Surface) -> &'static str {
    match surface {
        Surface::Xdr => "XDR",
        Surface::Rpc => "RPC",
        Surface::Soroban => "Soroban",
    }
}
