//! Aggregate result summary invariants (issue #822).
//!
//! `summarize` is public API for external consumers of `canary-runner`.
//! These tests pin its documented contract across heterogeneous result
//! slices:
//!
//! - `total` is `results.len()`, so a [`Status::Skipped`] result moves the
//!   total but increments none of `passed`, `failed`, `warnings` or `errors`.
//! - `passed_fraction` is the raw `(passed, total)` pair and handles empty,
//!   all-passed and all-skipped slices without dividing by zero.
//! - `has_required_failure` is true exactly when `failed > 0 || errors > 0`;
//!   warnings and skipped results never fail a run.
//! - Large slices are counted without overflow.

use canary_core::{CompatibilityResult, ProtocolVersion, ResultSource, Status, Surface};
use canary_runner::aggregation::{summarize, ResultSummary};

fn result(status: Status) -> CompatibilityResult {
    CompatibilityResult {
        test_id: format!("t-{status}"),
        protocol: ProtocolVersion(28),
        surface: Surface::Xdr,
        status,
        summary: "s".into(),
        details: None,
        duration_ms: 0,
        fixture_id: None,
        source: ResultSource::Live,
    }
}

fn results(statuses: &[Status]) -> Vec<CompatibilityResult> {
    statuses.iter().copied().map(result).collect()
}

#[test]
fn mixed_statuses_are_counted_independently() {
    let summary = summarize(&results(&[
        Status::Pass,
        Status::Pass,
        Status::Pass,
        Status::Fail,
        Status::Warning,
        Status::Warning,
        Status::Error,
    ]));

    assert_eq!(
        summary,
        ResultSummary {
            total: 7,
            passed: 3,
            failed: 1,
            warnings: 2,
            errors: 1,
        }
    );
}

#[test]
fn a_skipped_result_counts_toward_total_but_no_status_counter() {
    let summary = summarize(&results(&[
        Status::Pass,
        Status::Skipped,
        Status::Fail,
        Status::Skipped,
    ]));

    assert_eq!(summary.total, 4);
    assert_eq!(summary.passed, 1);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.warnings, 0);
    assert_eq!(summary.errors, 0);
    assert_eq!(
        summary.total,
        summary.passed + summary.failed + summary.warnings + summary.errors + 2,
        "the two skipped results account for the difference"
    );
}

#[test]
fn an_all_skipped_slice_has_a_total_and_no_verdicts() {
    let summary = summarize(&results(&[Status::Skipped; 5]));

    assert_eq!(summary.total, 5);
    assert_eq!(summary.passed, 0);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.warnings, 0);
    assert_eq!(summary.errors, 0);
    assert_eq!(summary.passed_fraction(), (0, 5));
    assert!(!summary.has_required_failure());
}

// -- passed_fraction -------------------------------------------------------

#[test]
fn passed_fraction_of_an_empty_slice_is_zero_over_zero() {
    let summary = summarize(&[]);

    assert_eq!(summary, ResultSummary::default());
    assert_eq!(summary.passed_fraction(), (0, 0));
    assert!(!summary.has_required_failure());
}

#[test]
fn passed_fraction_of_an_all_passed_slice_is_one_over_one() {
    let summary = summarize(&results(&[Status::Pass; 4]));

    assert_eq!(summary.passed_fraction(), (4, 4));
    assert_eq!(summary.total, 4);
    assert!(!summary.has_required_failure());
}

#[test]
fn passed_fraction_counts_skipped_results_in_the_denominator_only() {
    let summary = summarize(&results(&[
        Status::Pass,
        Status::Pass,
        Status::Skipped,
        Status::Skipped,
    ]));

    assert_eq!(summary.passed_fraction(), (2, 4));
}

#[test]
fn passed_fraction_of_all_skipped_is_zero_over_the_total() {
    let summary = summarize(&results(&[Status::Skipped; 3]));

    assert_eq!(summary.passed_fraction(), (0, 3));
}

// -- has_required_failure --------------------------------------------------

#[test]
fn has_required_failure_is_true_exactly_when_a_fail_or_error_is_present() {
    for &pass in &[0usize, 1, 2] {
        for &fail in &[0usize, 1] {
            for &warnings in &[0usize, 1, 2] {
                for &errors in &[0usize, 1] {
                    for &skipped in &[0usize, 1, 2] {
                        let mut statuses = Vec::new();
                        statuses.extend(std::iter::repeat_n(Status::Pass, pass));
                        statuses.extend(std::iter::repeat_n(Status::Fail, fail));
                        statuses.extend(std::iter::repeat_n(Status::Warning, warnings));
                        statuses.extend(std::iter::repeat_n(Status::Error, errors));
                        statuses.extend(std::iter::repeat_n(Status::Skipped, skipped));

                        let summary = summarize(&results(&statuses));
                        let expected = fail > 0 || errors > 0;

                        assert_eq!(
                            summary.has_required_failure(),
                            expected,
                            "pass={pass} fail={fail} warnings={warnings} errors={errors} skipped={skipped}"
                        );
                        assert_eq!(summary.total, statuses.len());
                        assert_eq!(summary.failed, fail);
                        assert_eq!(summary.errors, errors);
                    }
                }
            }
        }
    }
}

#[test]
fn warnings_alone_never_fail_a_run() {
    let summary = summarize(&results(&[
        Status::Warning,
        Status::Warning,
        Status::Pass,
        Status::Skipped,
    ]));

    assert_eq!(summary.warnings, 2);
    assert!(!summary.has_required_failure());
}

#[test]
fn a_single_error_among_passes_is_a_required_failure() {
    let summary = summarize(&results(&[Status::Pass, Status::Pass, Status::Error]));

    assert_eq!(summary.passed_fraction(), (2, 3));
    assert!(summary.has_required_failure());
}

// -- Large slices ----------------------------------------------------------

#[test]
fn large_slices_are_counted_without_overflow() {
    // `usize` counters: 200k results are nowhere near the limit, but the
    // count must still match exactly and the totals must stay consistent.
    let mut statuses = Vec::with_capacity(200_000);
    for index in 0..200_000 {
        statuses.push(match index % 5 {
            0 => Status::Pass,
            1 => Status::Pass,
            2 => Status::Fail,
            3 => Status::Warning,
            _ => Status::Skipped,
        });
    }

    let summary = summarize(&results(&statuses));

    assert_eq!(summary.total, 200_000);
    assert_eq!(summary.passed, 80_000);
    assert_eq!(summary.failed, 40_000);
    assert_eq!(summary.warnings, 40_000);
    assert_eq!(summary.errors, 0);
    assert_eq!(summary.passed_fraction(), (80_000, 200_000));
    assert!(summary.has_required_failure());

    // No counter wrapped: the accounted-for entries match the total.
    let skipped = 40_000;
    assert_eq!(
        summary.passed + summary.failed + summary.warnings + summary.errors + skipped,
        summary.total
    );
}

#[test]
fn an_all_passed_large_slice_never_reports_a_required_failure() {
    let summary = summarize(&results(&vec![Status::Pass; 100_000]));

    assert_eq!(summary.total, 100_000);
    assert_eq!(summary.passed, 100_000);
    assert_eq!(summary.passed_fraction(), (100_000, 100_000));
    assert!(!summary.has_required_failure());
}
