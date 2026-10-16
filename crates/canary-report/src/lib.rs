//! Terminal, JSON, and Markdown reporters for Stellar Protocol Canary.
//!
//! Every reporter consumes the same normalized [`ReportInput`] — none of
//! them re-run anything or compute their own pass/fail verdict; that is
//! already decided by `canary_core::PolicyEvaluator` before a report is
//! ever rendered.

pub mod diff;
pub mod json;
pub mod junit;
pub mod markdown;
pub mod terminal;
pub mod writer;

use canary_core::{
    CompatibilityResult, GitContext, NetworkName, PolicyDecision, ProjectType, ProtocolVersion,
    Surface,
};

pub use json::JsonReporter;
pub use junit::JUnitReporter;
pub use markdown::MarkdownReporter;
pub use terminal::TerminalReporter;
pub use writer::atomic_write_report;

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
    pub error: Option<String>,
}

/// What is known about a fixture's CF-04 verification evidence, for
/// display. Verification is a separate, dated, attributed event — a
/// passing run is not a verification (CF-04 section 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationSummary {
    pub fixture_id: String,
    /// The CF-04 method: `structural`, `source-checked`, `live-read-only`
    /// or `live-simulation`.
    pub method: String,
    /// `matched` or `mismatched`.
    pub outcome: String,
    /// The RFC 3339 UTC instant the record was checked at.
    pub checked_at: String,
}

impl VerificationSummary {
    /// The badge shown next to a result, e.g. `[verified: live-simulation]`.
    ///
    /// A `matched` record is `[verified: <method>]`; a `mismatched`
    /// record is reported as unverified and names its method, because a
    /// mismatched record is retained precisely to explain why the
    /// fixture is not trusted (CF-04 section 3).
    pub fn badge(&self) -> String {
        if self.outcome == "matched" {
            format!("[verified: {}]", self.method)
        } else {
            format!("[unverified: {} record {}]", self.method, self.outcome)
        }
    }
}

/// Facts about the engine run that produced a report (CF-01 `run`).
///
/// Deliberately limited to values that are the same for every run of one
/// binary: a build timestamp or a per-run clock reading would make two
/// otherwise identical runs differ byte for byte, which CF-01 section 4 rules
/// out for report version 1.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunSummary {
    /// The Git commit the running binary was built from, or `None` when it
    /// was built without Git history (a source archive, a packaged crate, or
    /// no `git` executable). `None` is rendered as JSON `null`, never as a
    /// placeholder string.
    pub tool_build_commit: Option<String>,
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
    /// Verification evidence for the fixtures of this run, by fixture
    /// id. A fixture missing from this list is unverified, which is the
    /// unknown state; the terminal reporter shows that rather than
    /// omitting it.
    pub verification: Vec<VerificationSummary>,
    pub decision: PolicyDecision,
    pub git: GitContext,
    pub verbose: bool,
    /// Build provenance of the engine. `None` means *not recorded* (a report
    /// written before the field existed), which is different from a
    /// recorded run whose commit is unknown.
    pub run: Option<RunSummary>,
}

impl ReportInput {
    /// The verification evidence recorded for `fixture_id`, if any.
    pub fn verification_for(&self, fixture_id: &str) -> Option<&VerificationSummary> {
        self.verification
            .iter()
            .find(|record| record.fixture_id == fixture_id)
    }

    /// The verification badge for one result: the fixture's recorded
    /// evidence, or `[unverified]` when there is none. Never empty — the
    /// unknown state is shown, not hidden.
    pub fn verification_badge(&self, result: &CompatibilityResult) -> String {
        match result
            .fixture_id
            .as_deref()
            .and_then(|id| self.verification_for(id))
        {
            Some(record) => record.badge(),
            None => "[unverified]".to_string(),
        }
    }
    /// How many results were replayed from the local result cache instead of
    /// being executed in this run.
    pub fn cached_count(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.source == canary_core::ResultSource::Cache)
            .count()
    }

    /// The sentence reporters add when [`ReportInput::cached_count`] is not
    /// zero, or `None`.
    pub fn cache_notice(&self) -> Option<String> {
        let cached = self.cached_count();
        (cached > 0).then(|| {
            format!(
                "{cached} of {} results were replayed from the local result cache and were not executed in this run. Use --no-cache for a fresh run.",
                self.results.len()
            )
        })
    }

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
    ///     source: canary_core::ResultSource::Live,
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
    ///     verification: Vec::new(),
    ///     decision: canary_core::PolicyDecision::Pass,
    ///     git: canary_core::GitContext::default(),
    ///     verbose: false,
    ///     run: None,
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

/// Maximum size, in bytes, of report JSON accepted from a stream such as
/// standard input (10 MiB — the report input cap approved under D-06 in
/// `docs/contracts/cf-05-report-comparison.md`).
pub const MAX_REPORT_JSON_BYTES: usize = 10 * 1024 * 1024;

/// Errors produced while reading report JSON from a stream.
#[derive(Debug, thiserror::Error)]
pub enum ReportReadError {
    /// The stream delivered more than [`MAX_REPORT_JSON_BYTES`] bytes.
    /// Reading stops at the limit instead of truncating the input, so the
    /// caller can never mistake a partial report for a complete one.
    #[error("report input exceeds the 10 MiB limit")]
    InputTooLarge,

    /// The stream could not be read to EOF, or its bytes were not UTF-8.
    #[error("failed to read report input: {0}")]
    Io(#[from] std::io::Error),
}

/// Reads report JSON from `reader` until EOF, enforcing
/// [`MAX_REPORT_JSON_BYTES`].
///
/// The limit is checked *before* buffering the overflowing chunk, so no
/// more than [`MAX_REPORT_JSON_BYTES`] bytes are ever held in memory and
/// an oversized stream fails fast with [`ReportReadError::InputTooLarge`]
/// instead of being accepted or silently cut short. The returned text is
/// exactly the bytes read (as UTF-8), ready for
/// [`JsonReporter::parse`].
///
/// # Examples
///
/// ```
/// use canary_report::read_report_json;
///
/// let json = read_report_json(&b"{\"a\": 1}"[..]).unwrap();
/// assert_eq!(json, "{\"a\": 1}");
///
/// let oversized = vec![b' '; 10 * 1024 * 1024 + 1];
/// let err = read_report_json(&oversized[..]).unwrap_err();
/// assert!(matches!(err, canary_report::ReportReadError::InputTooLarge));
/// ```
pub fn read_report_json<R: std::io::Read>(mut reader: R) -> Result<String, ReportReadError> {
    const CHUNK_BYTES: usize = 64 * 1024;

    let mut bytes: Vec<u8> = Vec::new();
    let mut chunk = [0_u8; CHUNK_BYTES];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if bytes.len() + read > MAX_REPORT_JSON_BYTES {
            return Err(ReportReadError::InputTooLarge);
        }
        bytes.extend_from_slice(&chunk[..read]);
    }

    String::from_utf8(bytes).map_err(|err| {
        ReportReadError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("input is not valid UTF-8: {err}"),
        ))
    })
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
            source: canary_core::ResultSource::Live,
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
            verification: Vec::new(),
            decision,
            git: GitContext::default(),
            verbose: false,
            run: None,
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

    #[test]
    fn read_report_json_reads_a_stream_to_eof() {
        // Larger than one internal read chunk, so this exercises the loop
        // that stitches chunks together rather than a single-read path.
        let mut payload = String::from("{\"schemaVersion\":1");
        payload.push_str(&" ".repeat(200_000));
        payload.push('}');

        let read = read_report_json(payload.as_bytes()).unwrap();
        assert_eq!(read, payload);
    }

    #[test]
    fn read_report_json_accepts_input_of_exactly_the_limit() {
        let exact = vec![b' '; MAX_REPORT_JSON_BYTES];
        let read = read_report_json(&exact[..]).unwrap();
        assert_eq!(read.len(), MAX_REPORT_JSON_BYTES);
    }

    #[test]
    fn read_report_json_rejects_input_larger_than_the_limit() {
        // One byte past the D-06 cap must abort with InputTooLarge rather
        // than being buffered whole or silently truncated.
        let oversized = vec![b' '; MAX_REPORT_JSON_BYTES + 1];
        let err = read_report_json(&oversized[..]).unwrap_err();
        assert!(
            matches!(err, ReportReadError::InputTooLarge),
            "unexpected error: {err}"
        );
        assert!(err.to_string().contains("10 MiB"), "{err}");
    }

    #[test]
    fn read_report_json_rejects_non_utf8_input() {
        let err = read_report_json(&[0xff, 0xfe, 0xfd][..]).unwrap_err();
        assert!(matches!(err, ReportReadError::Io(_)), "unexpected: {err}");
        assert!(err.to_string().contains("UTF-8"), "{err}");
    }

    #[test]
    fn read_report_json_propagates_reader_failures() {
        struct Failing;
        impl std::io::Read for Failing {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "reset",
                ))
            }
        }

        let err = read_report_json(Failing).unwrap_err();
        assert!(matches!(err, ReportReadError::Io(_)), "unexpected: {err}");
    }
}
