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
    /// Returns the run's results for one surface, in the order they appear
    /// in [`ReportInput::results`] — the order the runner executed them in.
    ///
    /// Reporters use this to render each surface's section without manually
    /// filtering; see [`SURFACE_ORDER`] for the fixed order surfaces are
    /// iterated in.
    ///
    /// The iterator borrows `self`, is empty when no result matches
    /// `surface`, and yields results for *that* surface only — including
    /// skipped ones, whose [`Status::Skipped`](canary_core::Status::Skipped) status is reported as-is
    /// (status handling is the reporter's job, not this method's).
    ///
    /// ```
    /// use canary_core::{ProtocolVersion, ProjectType, Surface};
    /// use canary_report::ReportInput;
    ///
    /// let xdr_pass = canary_core::CompatibilityResult {
    ///     test_id: "p28-xdr-cap83-empty-tx-set".into(),
    ///     protocol: ProtocolVersion(28),
    ///     surface: Surface::Xdr,
    ///     status: canary_core::Status::Pass,
    ///     summary: "StellarValue roundtrip".into(),
    ///     details: None,
    ///     duration_ms: 3,
    ///     fixture_id: Some("p28-xdr-cap83-empty-tx-set".into()),
    /// };
    /// let rpc_pass = canary_core::CompatibilityResult {
    ///     test_id: "p28-rpc-get-network".into(),
    ///     surface: Surface::Rpc,
    ///     ..xdr_pass.clone()
    /// };
    /// let report = ReportInput {
    ///     tool_version: "0.0.0-doc".into(),
    ///     target_protocol: ProtocolVersion(28),
    ///     project: canary_report::ProjectSummary {
    ///         name: "my-project".into(),
    ///         project_type: ProjectType::Soroban,
    ///     },
    ///     network: None,
    ///     results: vec![xdr_pass.clone(), rpc_pass, xdr_pass],
    ///     skipped: Vec::new(),
    ///     decision: canary_core::PolicyDecision::Pass,
    ///     git: canary_core::GitContext::default(),
    ///     verbose: false,
    /// };
    ///
    /// let xdr: Vec<_> = report.results_for(Surface::Xdr).collect();
    /// assert_eq!(xdr.len(), 2);
    /// assert!(xdr.iter().all(|r| r.surface == Surface::Xdr));
    /// // Results are yielded in execution order, not re-sorted.
    /// assert_eq!(xdr[0].test_id, "p28-xdr-cap83-empty-tx-set");
    /// assert_eq!(report.results_for(Surface::Soroban).count(), 0);
    /// ```
    pub fn results_for(&self, surface: Surface) -> impl Iterator<Item = &CompatibilityResult> {
        self.results.iter().filter(move |r| r.surface == surface)
    }

    /// Returns `true` if any of the results in this report encountered an execution error.
    ///
    /// An [`Error`](canary_core::Status::Error) status indicates that a test fixture failed
    /// to execute correctly (e.g., due to a panic, a missing file, or a timeout). This is
    /// distinct from a normal test failure, which is represented by a `Fail` status.
    ///
    /// **Important:** When this method returns `true`, the overall report status is
    /// unconditionally escalated to [`ReportStatus::Error`], overriding whatever
    /// [`PolicyDecision`] the planner originally issued.
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

#[cfg(test)]
mod tests {
    use canary_core::{ProjectType, Status, Surface};

    use super::*;

    fn result(status: Status) -> CompatibilityResult {
        CompatibilityResult {
            test_id: "t1".into(),
            protocol: ProtocolVersion(24),
            surface: Surface::Xdr,
            status,
            summary: "mock".into(),
            details: None,
            duration_ms: 0,
            fixture_id: None,
        }
    }

    fn input(decision: PolicyDecision, results: Vec<CompatibilityResult>) -> ReportInput {
        ReportInput {
            tool_version: "0.0.0-test".into(),
            target_protocol: ProtocolVersion(24),
            project: ProjectSummary {
                name: "mock-project".into(),
                project_type: ProjectType::Unknown,
            },
            network: None,
            results,
            skipped: Vec::new(),
            decision,
            git: GitContext::default(),
            verbose: false,
        }
    }

    #[test]
    fn overall_status_reports_error_even_when_decision_is_pass() {
        let report = input(
            PolicyDecision::Pass,
            vec![result(Status::Pass), result(Status::Error)],
        );
        assert!(report.has_any_error());
        assert_eq!(report.overall_status(), ReportStatus::Error);
    }

    #[test]
    fn overall_status_falls_back_to_decision_without_errors() {
        let cases = [
            (PolicyDecision::Pass, ReportStatus::Pass),
            (PolicyDecision::Warning, ReportStatus::Warning),
            (PolicyDecision::Fail, ReportStatus::Fail),
        ];
        for (decision, expected) in cases {
            let report = input(decision, vec![result(Status::Pass), result(Status::Fail)]);
            assert!(!report.has_any_error());
            assert_eq!(report.overall_status(), expected);
        }
    }

    #[test]
    fn overall_status_with_no_results_uses_decision() {
        let report = input(PolicyDecision::Pass, Vec::new());
        assert_eq!(report.overall_status(), ReportStatus::Pass);
    }

    #[test]
    fn report_status_as_str_maps_to_lowercase_forms() {
        assert_eq!(ReportStatus::Pass.as_str(), "pass");
        assert_eq!(ReportStatus::Warning.as_str(), "warning");
        assert_eq!(ReportStatus::Fail.as_str(), "fail");
        assert_eq!(ReportStatus::Error.as_str(), "error");
    }

    #[test]
    fn results_for_filters_by_surface() {
        let mut rpc_failure = result(Status::Fail);
        rpc_failure.surface = Surface::Rpc;
        let report = input(
            PolicyDecision::Fail,
            vec![result(Status::Pass), rpc_failure],
        );

        let xdr: Vec<_> = report.results_for(Surface::Xdr).collect();
        let rpc: Vec<_> = report.results_for(Surface::Rpc).collect();
        assert_eq!(xdr.len(), 1);
        assert_eq!(rpc.len(), 1);
        assert_eq!(rpc[0].surface, Surface::Rpc);
    }
}
