//! Summarizing a set of [`CompatibilityResult`]s.

use canary_core::{CompatibilityResult, Status};

/// Counts of results by status, over the fixtures that actually ran.
///
/// Skipped fixtures are never included here: they are neither pass nor
/// fail and are tracked separately by the planner.
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
}
