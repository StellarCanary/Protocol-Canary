//! Summarizing a set of [`CompatibilityResult`]s.
//!
//! This module is public API intended for **external consumers of
//! `canary-runner`** — for example a CI integration or dashboard that runs
//! the scheduler itself and wants aggregate counts without depending on
//! the `canary-cli` binary. The CLI's own reporters deliberately do not
//! use it: `canary-report` computes its JSON counts independently so that
//! it never depends on this crate (see the comment on `JsonCounts` in
//! `canary-report/src/json.rs`). The two count structures are kept in sync
//! by hand.

use canary_core::{CompatibilityResult, Status};

/// Counts of results by status, over the fixtures that actually ran.
///
/// Skipped fixtures are never included here: they are neither pass nor
/// fail and are tracked separately by the planner.
///
/// # Examples
///
/// ```
/// use canary_runner::summarize;
///
/// let summary = summarize(&[]);
/// assert_eq!(summary.passed_fraction(), (0, 0));
/// assert!(!summary.has_required_failure());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResultSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub warnings: usize,
    pub errors: usize,
}

impl ResultSummary {
    /// `passed / total`, as the two integers, never a fabricated
    /// percentage with an undefined denominator.
    pub fn passed_fraction(&self) -> (usize, usize) {
        (self.passed, self.total)
    }

    pub fn has_required_failure(&self) -> bool {
        self.failed > 0 || self.errors > 0
    }
}

/// Aggregates a slice of test results into a [`ResultSummary`].
///
/// Counts the occurrences of each [`Status`] across the provided `results`,
/// recording the counts of [`Pass`](Status::Pass), [`Fail`](Status::Fail),
/// [`Warning`](Status::Warning), and [`Error`](Status::Error) outcomes.
///
/// The returned [`ResultSummary::total`] is initialized to `results.len()`.
/// If the slice contains any [`Status::Skipped`] results, they are included
/// in `total` but do not increment any of the four status-specific counters,
/// as skipped fixtures are normally filtered and tracked during planning
/// rather than execution.
///
/// If `results` is empty, this returns a default [`ResultSummary`] where all
/// counts are zero.
///
/// This function performs a single linear pass over `results`, does not allocate,
/// and never panics.
///
/// # Examples
///
/// ```
/// use canary_core::{CompatibilityResult, ProtocolVersion, Status, Surface};
/// use canary_runner::{summarize, ResultSummary};
///
/// let results = vec![
///     CompatibilityResult {
///         test_id: "xdr-01".into(),
///         protocol: ProtocolVersion(28),
///         surface: Surface::Xdr,
///         status: Status::Pass,
///         summary: "decoded successfully".into(),
///         details: None,
///         duration_ms: 5,
///         fixture_id: None,
///         source: canary_core::ResultSource::Live,
///     },
///     CompatibilityResult {
///         test_id: "rpc-01".into(),
///         protocol: ProtocolVersion(28),
///         surface: Surface::Rpc,
///         status: Status::Fail,
///         summary: "assertion failed".into(),
///         details: None,
///         duration_ms: 12,
///         fixture_id: None,
///         source: canary_core::ResultSource::Live,
///     },
/// ];
///
/// let summary = summarize(&results);
/// assert_eq!(summary.total, 2);
/// assert_eq!(summary.passed, 1);
/// assert_eq!(summary.failed, 1);
/// assert_eq!(summary.warnings, 0);
/// assert_eq!(summary.errors, 0);
/// assert_eq!(summary.passed_fraction(), (1, 2));
/// assert!(summary.has_required_failure());
/// ```
pub fn summarize(results: &[CompatibilityResult]) -> ResultSummary {
    let mut summary = ResultSummary {
        total: results.len(),
        ..ResultSummary::default()
    };
    for result in results {
        match result.status {
            Status::Pass => summary.passed += 1,
            Status::Fail => summary.failed += 1,
            Status::Warning => summary.warnings += 1,
            Status::Error => summary.errors += 1,
            Status::Skipped => {}
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use canary_core::{ProtocolVersion, Surface};

    fn result(status: Status) -> CompatibilityResult {
        CompatibilityResult {
            test_id: "t".into(),
            protocol: ProtocolVersion(28),
            surface: Surface::Xdr,
            status,
            summary: "s".into(),
            details: None,
            duration_ms: 0,
            fixture_id: None,
            source: canary_core::ResultSource::Live,
        }
    }

    #[test]
    fn counts_each_status_independently() {
        let results = vec![
            result(Status::Pass),
            result(Status::Pass),
            result(Status::Fail),
            result(Status::Warning),
            result(Status::Error),
        ];
        let summary = summarize(&results);
        assert_eq!(summary.total, 5);
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.warnings, 1);
        assert_eq!(summary.errors, 1);
    }

    #[test]
    fn empty_results_are_not_a_required_failure() {
        assert!(!summarize(&[]).has_required_failure());
    }

    #[test]
    fn a_failure_or_error_counts_as_a_required_failure() {
        assert!(summarize(&[result(Status::Fail)]).has_required_failure());
        assert!(summarize(&[result(Status::Error)]).has_required_failure());
        assert!(!summarize(&[result(Status::Warning)]).has_required_failure());
    }

    #[test]
    fn passed_fraction_reports_passed_over_total_for_a_mixed_summary() {
        let results = vec![
            result(Status::Pass),
            result(Status::Pass),
            result(Status::Fail),
            result(Status::Warning),
            result(Status::Error),
        ];
        assert_eq!(summarize(&results).passed_fraction(), (2, 5));
    }

    #[test]
    fn passed_fraction_of_an_empty_result_set_is_zero_over_zero() {
        // Reporters render this as "0/0"; a percentage here would have an
        // undefined denominator, which is why the accessor returns integers.
        assert_eq!(summarize(&[]).passed_fraction(), (0, 0));
    }
}
