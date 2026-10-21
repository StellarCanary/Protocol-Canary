//! Default human-readable terminal output.

use std::fmt::Write as _;

use canary_core::Status;

use crate::diff::{Classification, Comparison, State, Verdict};
use crate::{surface_heading, ReportInput, ReportStatus, SURFACE_ORDER};

const RULE: &str = "────────────────────────────────────────";

fn status_badge(status: Status) -> &'static str {
    match status {
        Status::Pass => "✓ PASS",
        Status::Fail => "❌ FAIL",
        Status::Warning => "⚠ WARN",
        Status::Error => "‼ ERROR",
        Status::Skipped => "− SKIPPED",
    }
}

fn decision_label(input: &ReportInput) -> &'static str {
    match input.overall_status() {
        ReportStatus::Pass => "PASS",
        ReportStatus::Warning => "WARNING",
        ReportStatus::Fail => "NOT READY",
        ReportStatus::Error => "ERROR",
    }
}

/// Renders [`ReportInput`] as the default terminal report.
pub struct TerminalReporter;

impl TerminalReporter {
    /// Renders a given [`ReportInput`] into a human-readable terminal-friendly string.
    ///
    /// The generated report includes project and protocol metadata, detailed test results
    /// grouped by surface, a summary of skipped fixtures, and an overall compatibility decision.
    /// Any failures or errors are listed at the end with their associated details.
    ///
    /// # Arguments
    ///
    /// * `input` - The structured [`ReportInput`] containing all results and metadata to report.
    ///
    /// # Returns
    ///
    /// A formatted `String` ready to be printed to `stdout` or `stderr`.
    ///
    /// # Examples
    ///
    /// ```text
    /// let text = TerminalReporter::render(&report_input);
    /// println!("{}", text);
    /// ```
    pub fn render(input: &ReportInput) -> String {
        let mut out = String::new();

        let _ = writeln!(out, "Stellar Protocol Canary");
        let _ = writeln!(out, "{RULE}");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Project: {} ({})",
            input.project.name, input.project.project_type
        );
        let _ = writeln!(out, "Target protocol: {}", input.target_protocol);
        if let Some(network) = &input.network {
            match network.observed_protocol {
                Some(observed) => {
                    let _ = writeln!(
                        out,
                        "Network: {} (observed protocol {observed})",
                        network.name
                    );
                }
                None => {
                    if let Some(err) = &network.error {
                        let _ = writeln!(
                            out,
                            "Network: {} (protocol not observed: {err})",
                            network.name
                        );
                    } else {
                        let _ = writeln!(out, "Network: {} (protocol not observed)", network.name);
                    }
                }
            }
        }
        let _ = writeln!(out);

        for surface in SURFACE_ORDER {
            let results: Vec<_> = input.results_for(surface).collect();
            if results.is_empty() {
                continue;
            }
            let _ = writeln!(out, "{}", surface_heading(surface));
            let all_passed = results.iter().all(|r| r.status == Status::Pass);
            // The compact `N/N PASS` line stays for runs that carry no
            // verification evidence; when evidence exists, every result
            // gets a row so its verification badge can be shown (a
            // fixture without evidence shows `[unverified]` rather than
            // silently disappearing).
            let show_verification = !input.verification.is_empty();
            if all_passed && !show_verification {
                let _ = writeln!(out, "  {}/{} PASS", results.len(), results.len());
            } else {
                for result in &results {
                    if show_verification {
                        let _ = writeln!(
                            out,
                            "  {:<28} {} {}",
                            result.test_id,
                            status_badge(result.status),
                            input.verification_badge(result)
                        );
                    } else {
                        let _ = writeln!(
                            out,
                            "  {:<28} {}",
                            result.test_id,
                            status_badge(result.status)
                        );
                    }
                }
            }
            let _ = writeln!(out);
        }

        if !input.skipped.is_empty() {
            let _ = writeln!(out, "Skipped fixtures: {}", input.skipped.len());
            if input.verbose {
                for skip in &input.skipped {
                    let _ = writeln!(
                        out,
                        "  {} ({}): {}",
                        skip.fixture_id, skip.surface, skip.reason
                    );
                }
            }
            let _ = writeln!(out);
        }

        let _ = writeln!(out, "{RULE}");
        let _ = writeln!(out);

        let passed = input
            .results
            .iter()
            .filter(|r| r.status == Status::Pass)
            .count();
        let total = input.results.len();
        let _ = writeln!(out, "{passed}/{total} applicable checks passed.");
        if let Some(notice) = input.cache_notice() {
            let _ = writeln!(out, "{notice}");
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "Status: {}", decision_label(input));

        let failures: Vec<_> = input
            .results
            .iter()
            .filter(|r| matches!(r.status, Status::Fail | Status::Error))
            .collect();
        if !failures.is_empty() {
            let _ = writeln!(out);
            for failure in failures {
                let _ = writeln!(out, "Failure:");
                let _ = writeln!(out, "{}", failure.test_id);
                let _ = writeln!(out);
                let _ = writeln!(out, "{}", failure.summary);
                if let Some(details) = &failure.details {
                    let _ = writeln!(out);
                    let _ = writeln!(out, "{details}");
                }
                let _ = writeln!(out);
            }
        }

        out
    }
}

/// The terminal badge for one [`Classification`], the `[NAME]` form. The
/// badge-only distinction is what colour adds on top of; without colour the
/// badge spelling is the whole signal.
fn classification_badge(classification: Classification) -> &'static str {
    match classification {
        Classification::Unchanged => "[UNCHANGED]",
        Classification::Degraded => "[DEGRADED]",
        Classification::Regressed => "[REGRESSED]",
        Classification::Improved => "[IMPROVED]",
        Classification::Resolved => "[RESOLVED]",
        Classification::Added => "[ADDED]",
        Classification::AddedFailing => "[ADDED-FAILING]",
        Classification::Removed => "[REMOVED]",
        Classification::RemovedFailing => "[REMOVED-FAILING]",
        Classification::Skipped => "[SKIPPED]",
        Classification::SkippedFailing => "[SKIPPED-FAILING]",
        Classification::UnchangedSkip => "[UNCHANGED-SKIP]",
    }
}

/// The badge's width without its ANSI wrapping, so rows stay aligned whether
/// or not colour is on: with colour the escape bytes would otherwise be
/// counted by `{:<width$}` and shorten the visual pad.
const BADGE_WIDTH: usize = "[REMOVED-FAILING]".len();

/// Colour for one [`Classification`] (short SGR parameter only; the caller
/// writes reset). `None` renders plain.
fn classification_color(classification: Classification) -> Option<&'static str> {
    match classification {
        Classification::Regressed
        | Classification::AddedFailing
        | Classification::SkippedFailing => Some("1;31"),
        Classification::Degraded | Classification::RemovedFailing => Some("33"),
        Classification::Improved | Classification::Resolved | Classification::Added => Some("32"),
        Classification::Unchanged
        | Classification::Removed
        | Classification::Skipped
        | Classification::UnchangedSkip => None,
    }
}

/// The label for one [`Verdict`], and its colour: the verdict is the first
/// line a CI reader scans.
fn verdict_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Regression => "REGRESSION",
        Verdict::CoverageLost => "COVERAGE LOST",
        Verdict::Improved => "IMPROVED",
        Verdict::NoChange => "NO CHANGE",
    }
}

fn verdict_color(verdict: Verdict) -> Option<&'static str> {
    match verdict {
        Verdict::Regression => Some("1;31"),
        Verdict::CoverageLost => Some("33"),
        Verdict::Improved => Some("32"),
        Verdict::NoChange => None,
    }
}

/// The lowercase state name a comparison carries, as `diffVersion` 1 stores
/// it (CF-05 section 5); the arrow column reuses it verbatim.
fn state_label(state: State) -> &'static str {
    match state {
        State::Pass => "pass",
        State::Warning => "warning",
        State::Fail => "fail",
        State::Error => "error",
        State::Skipped => "skipped",
        State::Absent => "absent",
    }
}

/// Whether the environment asks for colour to be dropped.
///
/// `NO_COLOR` is respected per the convention at <https://no-color.org>: set
/// to a non-empty value, colour is suppressed. Colour is enabled
/// unconditionally otherwise — the renderer is as likely to run into a file
/// (a CI step summary) as into a terminal, and catching a non-tty here would
/// break `stellar-canary check > report.txt` for no benefit; the caller
/// decides where the string goes.
fn color_from_env() -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
}

/// Wraps `text` in one SGR pair when `color_on` and a colour exists.
fn colored(text: &str, color: Option<&'static str>, color_on: bool) -> String {
    match (color, color_on) {
        (Some(sgr), true) => format!("\u{1b}[{sgr}m{text}\u{1b}[0m"),
        _ => text.to_string(),
    }
}

/// Wraps `badge` in one SGR pair, padding the *visible* text to
/// [`BADGE_WIDTH`] either way so rows stay aligned: with colour the escape
/// bytes would otherwise be counted by `{:<width$}` and shorten the visual
/// pad.
fn colored_badge(badge: &str, color: Option<&'static str>, color_on: bool) -> String {
    colored(&format!("{badge:<BADGE_WIDTH$}"), color, color_on)
}

impl TerminalReporter {
    /// Renders a report comparison (CF-05) as a terminal-friendly string.
    ///
    /// Colour is decided once from the environment: it is suppressed when
    /// `NO_COLOR` is set to a non-empty value and enabled otherwise. Code
    /// that needs to pin the decision (tests, a future `--no-color` flag)
    /// calls [`TerminalReporter::render_comparison_with_color`] instead.
    ///
    /// The output is deterministic for a given [`Comparison`]: no
    /// timestamps, no paths, entries in the comparison's own
    /// `(surface, id)` order.
    ///
    /// # Examples
    ///
    /// ```text
    /// let text = TerminalReporter::render_comparison(&comparison);
    /// println!("{text}");
    /// ```
    pub fn render_comparison(comparison: &Comparison) -> String {
        Self::render_comparison_with_color(comparison, color_from_env())
    }

    /// Renders a comparison with an explicit colour decision, so callers
    /// (and tests) can pin whether ANSI colour sequences appear at all.
    ///
    /// The layout, badges, notes and ordering are exactly those of
    /// [`TerminalReporter::render_comparison`] — only the wrapping differs.
    pub fn render_comparison_with_color(comparison: &Comparison, color: bool) -> String {
        let mut out = String::new();

        let _ = writeln!(
            out,
            "Report comparison (diffVersion {})",
            comparison.diff_version
        );
        let _ = writeln!(out, "{RULE}");
        let _ = writeln!(
            out,
            "Baseline: {}, protocol {}, status {}",
            comparison.baseline.tool_version,
            comparison.baseline.target_protocol,
            comparison.baseline.status
        );
        let _ = writeln!(
            out,
            "Current:  {}, protocol {}, status {}",
            comparison.current.tool_version,
            comparison.current.target_protocol,
            comparison.current.status
        );
        let _ = writeln!(out,);

        let badge = colored(
            &format!("[{}]", verdict_label(comparison.verdict)),
            verdict_color(comparison.verdict),
            color,
        );
        let _ = writeln!(out, "Verdict: {badge}");

        let summary = &comparison.summary;
        let _ = writeln!(
            out,
            "Checks: {} regressed, {} resolved, {} added, {} removed, {} unchanged",
            summary.regressed, summary.resolved, summary.added, summary.removed, summary.unchanged
        );
        let _ = writeln!(out);

        if comparison.empty_baseline {
            let _ = writeln!(
                out,
                "Note: the baseline report is empty; no change can be attributed to it."
            );
        }
        if comparison.empty_current {
            let _ = writeln!(
                out,
                "Note: the current report is empty; no result can be judged against it."
            );
        }
        if comparison.observed_protocol_changed {
            let _ = writeln!(
                out,
                "Note: the reports observed different live protocol versions."
            );
        }
        if comparison.empty_baseline
            || comparison.empty_current
            || comparison.observed_protocol_changed
        {
            let _ = writeln!(out);
        }

        let _ = writeln!(out, "Checks:");
        for entry in &comparison.entries {
            let badge = colored_badge(
                classification_badge(entry.classification),
                classification_color(entry.classification),
                color,
            );
            let _ = writeln!(
                out,
                "  {} {:<28} {} → {}",
                badge,
                entry.id,
                state_label(entry.from),
                state_label(entry.to)
            );
        }
        if comparison.entries.is_empty() {
            let _ = writeln!(out, "  (no checks to compare)");
        }
        let _ = writeln!(out);

        if !comparison.missing.is_empty() {
            let _ = writeln!(out, "Missing checks ({}):", comparison.missing.len());
            for missing in &comparison.missing {
                // A check that passed in the baseline and stopped running is
                // how a regression hides, so the list stays explicit rather
                // than a bare id.
                let _ = writeln!(
                    out,
                    "  {}/{} passed in the baseline and is not running now",
                    missing.surface, missing.id
                );
            }
            let _ = writeln!(out);
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectSummary, ReportInput, VerificationSummary};
    use canary_core::{GitContext, PolicyDecision, ProjectType, ProtocolVersion};

    fn result(
        id: &str,
        surface: canary_core::Surface,
        status: Status,
    ) -> canary_core::CompatibilityResult {
        canary_core::CompatibilityResult {
            test_id: id.into(),
            protocol: ProtocolVersion(28),
            surface,
            status,
            summary: format!("{id} summary"),
            details: None,
            duration_ms: 1,
            fixture_id: Some(id.into()),
            source: canary_core::ResultSource::Live,
        }
    }

    fn base_input(
        results: Vec<canary_core::CompatibilityResult>,
        decision: PolicyDecision,
    ) -> ReportInput {
        ReportInput {
            tool_version: "0.1.0".into(),
            target_protocol: ProtocolVersion(28),
            project: ProjectSummary {
                name: "example".into(),
                project_type: ProjectType::Soroban,
            },
            network: None,
            results,
            skipped: vec![],
            verification: vec![],
            decision,
            git: GitContext::default(),
            verbose: false,
            run: None,
        }
    }

    fn verification(fixture_id: &str, method: &str, outcome: &str) -> VerificationSummary {
        VerificationSummary {
            fixture_id: fixture_id.into(),
            method: method.into(),
            outcome: outcome.into(),
            checked_at: "2026-09-28T07:20:00Z".into(),
        }
    }

    #[test]
    fn all_passing_renders_aggregate_counts_and_pass_status() {
        let input = base_input(
            vec![
                result("p28-xdr-1", canary_core::Surface::Xdr, Status::Pass),
                result("p28-xdr-2", canary_core::Surface::Xdr, Status::Pass),
            ],
            PolicyDecision::Pass,
        );
        let text = TerminalReporter::render(&input);
        assert!(text.contains("2/2 PASS"));
        assert!(text.contains("2/2 applicable checks passed."));
        assert!(text.contains("Status: PASS"));
        assert!(!text.contains("Failure:"));
    }

    #[test]
    fn a_failure_is_listed_individually_with_its_details() {
        let mut failing = result("p28-xdr-1", canary_core::Surface::Xdr, Status::Fail);
        failing.details = Some("decode mismatch".to_string());
        let input = base_input(vec![failing], PolicyDecision::Fail);
        let text = TerminalReporter::render(&input);
        assert!(text.contains("p28-xdr-1"));
        assert!(text.contains("FAIL"));
        assert!(text.contains("Status: NOT READY"));
        assert!(text.contains("Failure:"));
        assert!(text.contains("decode mismatch"));
    }

    #[test]
    fn an_execution_error_reports_error_status_even_with_a_pass_decision() {
        let input = base_input(
            vec![result(
                "p28-rpc-1",
                canary_core::Surface::Rpc,
                Status::Error,
            )],
            PolicyDecision::Pass,
        );
        let text = TerminalReporter::render(&input);
        assert!(text.contains("Status: ERROR"));
    }

    #[test]
    fn skipped_fixtures_are_summarized() {
        let mut input = base_input(vec![], PolicyDecision::Pass);
        input.skipped.push(crate::SkipSummary {
            fixture_id: "p28-soroban-1".into(),
            surface: canary_core::Surface::Soroban,
            reason: "requires a capability not declared by this project".into(),
            code: canary_core::SkipCode::MissingCapability,
        });
        let text = TerminalReporter::render(&input);
        assert!(text.contains("Skipped fixtures: 1"));
    }

    fn skip(
        fixture_id: &str,
        surface: canary_core::Surface,
        reason: &str,
        code: canary_core::SkipCode,
    ) -> crate::SkipSummary {
        crate::SkipSummary {
            fixture_id: fixture_id.into(),
            surface,
            reason: reason.into(),
            code,
        }
    }

    #[test]
    fn verbose_output_lists_each_skipped_fixture_with_its_reason() {
        let mut input = base_input(vec![], PolicyDecision::Pass);
        input.verbose = true;
        input.skipped.push(skip(
            "p28-soroban-1",
            canary_core::Surface::Soroban,
            "requires a capability not declared by this project",
            canary_core::SkipCode::MissingCapability,
        ));
        input.skipped.push(skip(
            "p28-rpc-2",
            canary_core::Surface::Rpc,
            "no RPC endpoint configured",
            canary_core::SkipCode::Unknown,
        ));
        let text = TerminalReporter::render(&input);
        assert!(text.contains("Skipped fixtures: 2"));
        assert!(text.contains(
            "p28-soroban-1 (soroban): requires a capability not declared by this project"
        ));
        assert!(text.contains("p28-rpc-2 (rpc): no RPC endpoint configured"));
    }

    #[test]
    fn non_verbose_output_only_prints_the_skipped_count() {
        let mut input = base_input(vec![], PolicyDecision::Pass);
        input.skipped.push(skip(
            "p28-soroban-1",
            canary_core::Surface::Soroban,
            "requires a capability not declared by this project",
            canary_core::SkipCode::MissingCapability,
        ));
        input.skipped.push(skip(
            "p28-rpc-2",
            canary_core::Surface::Rpc,
            "no RPC endpoint configured",
            canary_core::SkipCode::Unknown,
        ));
        let text = TerminalReporter::render(&input);
        assert!(text.contains("Skipped fixtures: 2"));
        assert!(!text.contains("requires a capability not declared by this project"));
        assert!(!text.contains("no RPC endpoint configured"));
        assert!(!text.contains("p28-soroban-1 ("));
        assert!(!text.contains("p28-rpc-2 ("));
    }

    #[test]
    fn a_run_with_replayed_results_says_so() {
        let mut cached = result("p28-xdr-1", canary_core::Surface::Xdr, Status::Pass);
        cached.source = canary_core::ResultSource::Cache;
        let live = result("p28-xdr-2", canary_core::Surface::Xdr, Status::Pass);
        let text = TerminalReporter::render(&base_input(vec![cached, live], PolicyDecision::Pass));
        assert!(text.contains("1 of 2 results were replayed from the local result cache"));
    }

    #[test]
    fn a_fully_live_run_has_no_cache_notice() {
        let live = result("p28-xdr-1", canary_core::Surface::Xdr, Status::Pass);
        let text = TerminalReporter::render(&base_input(vec![live], PolicyDecision::Pass));
        assert!(!text.contains("result cache"));
    }

    #[test]
    fn a_verified_fixture_shows_its_verification_badge() {
        let mut input = base_input(
            vec![result("p28-xdr-1", canary_core::Surface::Xdr, Status::Pass)],
            PolicyDecision::Pass,
        );
        input
            .verification
            .push(verification("p28-xdr-1", "live-simulation", "matched"));
        let text = TerminalReporter::render(&input);
        assert!(text.contains("[verified: live-simulation]"), "{text}");
        // With evidence present even a fully passing surface lists its
        // results, so the badge has somewhere to appear.
        assert!(text.contains("p28-xdr-1"), "{text}");
    }

    #[test]
    fn an_unverified_fixture_shows_an_unverified_indicator() {
        let mut input = base_input(
            vec![
                result("p28-xdr-1", canary_core::Surface::Xdr, Status::Pass),
                result("p28-xdr-2", canary_core::Surface::Xdr, Status::Pass),
            ],
            PolicyDecision::Pass,
        );
        input
            .verification
            .push(verification("p28-xdr-1", "source-checked", "matched"));
        let text = TerminalReporter::render(&input);
        assert!(text.contains("[verified: source-checked]"), "{text}");
        assert!(text.contains("[unverified]"), "{text}");
        // The unverified result keeps its own row, not silence.
        assert!(
            text.contains("p28-xdr-2") && text.contains("✓ PASS"),
            "{text}"
        );
    }

    #[test]
    fn a_mismatched_record_is_shown_as_unverified_naming_its_method() {
        let mut input = base_input(
            vec![result("p28-rpc-1", canary_core::Surface::Rpc, Status::Pass)],
            PolicyDecision::Pass,
        );
        input
            .verification
            .push(verification("p28-rpc-1", "live-read-only", "mismatched"));
        let text = TerminalReporter::render(&input);
        assert!(
            text.contains("[unverified: live-read-only record mismatched]"),
            "{text}"
        );
        assert!(!text.contains("[verified:"), "{text}");
    }

    #[test]
    fn a_run_without_verification_records_keeps_the_compact_pass_line() {
        let input = base_input(
            vec![
                result("p28-xdr-1", canary_core::Surface::Xdr, Status::Pass),
                result("p28-xdr-2", canary_core::Surface::Xdr, Status::Pass),
            ],
            PolicyDecision::Pass,
        );
        let text = TerminalReporter::render(&input);
        assert!(text.contains("2/2 PASS"), "{text}");
        assert!(!text.contains("[unverified]"), "{text}");
    }

    #[test]
    fn a_failure_row_is_unchanged_when_the_run_carries_no_verification() {
        let mut failing = result("p28-xdr-1", canary_core::Surface::Xdr, Status::Fail);
        failing.details = Some("decode mismatch".to_string());
        let text = TerminalReporter::render(&base_input(vec![failing], PolicyDecision::Fail));
        assert!(text.contains("p28-xdr-1"), "{text}");
        assert!(!text.contains("[unverified]"), "{text}");
    }

    mod comparison {
        use super::*;
        use crate::diff::{compare, CompareOptions};
        use canary_core::{
            CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, Surface,
        };

        fn result(id: &str, surface: Surface, status: Status) -> CompatibilityResult {
            CompatibilityResult {
                test_id: id.into(),
                protocol: ProtocolVersion(28),
                surface,
                status,
                summary: format!("{id} summary"),
                details: None,
                duration_ms: 1,
                fixture_id: Some(id.into()),
                source: canary_core::ResultSource::Live,
            }
        }

        fn report(results: Vec<CompatibilityResult>) -> ReportInput {
            report_with_decision(results, PolicyDecision::Pass)
        }

        fn report_with_decision(
            results: Vec<CompatibilityResult>,
            decision: PolicyDecision,
        ) -> ReportInput {
            ReportInput {
                tool_version: "0.2.0".into(),
                target_protocol: ProtocolVersion(28),
                project: ProjectSummary {
                    name: "example".into(),
                    project_type: ProjectType::Soroban,
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

        fn regression_comparison() -> crate::diff::Comparison {
            let baseline = report(vec![
                result("rpc-kept", Surface::Rpc, Status::Pass),
                result("rpc-broke", Surface::Rpc, Status::Pass),
                result("rpc-fixed", Surface::Rpc, Status::Fail),
            ]);
            let current = report_with_decision(
                vec![
                    result("rpc-kept", Surface::Rpc, Status::Pass),
                    result("rpc-broke", Surface::Rpc, Status::Fail),
                    result("rpc-fixed", Surface::Rpc, Status::Pass),
                ],
                PolicyDecision::Fail,
            );
            compare(&baseline, &current, CompareOptions::default()).unwrap()
        }

        #[test]
        fn a_regression_renders_a_red_badge_and_the_verdict_line() {
            let comparison = regression_comparison();
            let colored = TerminalReporter::render_comparison_with_color(&comparison, true);
            assert!(
                colored.contains("Verdict: \u{1b}[1;31m[REGRESSION]\u{1b}[0m"),
                "{colored}"
            );
            // The regressed check: pass -> fail, with its red badge.
            assert!(
                colored.contains("\u{1b}[1;31m[REGRESSED]      \u{1b}[0m rpc-broke"),
                "{colored}"
            );
            assert!(colored.contains("pass → fail"), "{colored}");
            // The resolved check gets a green badge.
            assert!(colored.contains("\u{1b}[32m[RESOLVED]"), "{colored}");
            assert!(colored.contains("fail → pass"), "{colored}");

            let plain = TerminalReporter::render_comparison_with_color(&comparison, false);
            assert!(plain.contains("Verdict: [REGRESSION]"), "{plain}");
            assert!(plain.contains("[REGRESSED]       rpc-broke"), "{plain}");
            assert!(
                !plain.contains('\u{1b}'),
                "plain render carries no escapes: {plain}"
            );
        }

        #[test]
        fn the_no_color_environment_suppresses_ansi_codes() {
            let comparison = regression_comparison();
            // Heads-up on mechanics: environment variables are process-global,
            // so this test would race any test that reads `NO_COLOR` while it
            // runs. The other tests here render through
            // `render_comparison_with_color`, which does not touch the
            // environment, and this is the only test that does.
            std::env::set_var("NO_COLOR", "1");
            let text = TerminalReporter::render_comparison(&comparison);
            std::env::remove_var("NO_COLOR");
            assert!(
                !text.contains('\u{1b}'),
                "NO_COLOR must suppress every ANSI sequence: {text}"
            );
            assert!(text.contains("Verdict: [REGRESSION]"), "{text}");
            assert!(text.contains("[REGRESSED]"), "{text}");
        }

        #[test]
        fn neutral_transitions_are_listed_without_emphasis() {
            let comparison = regression_comparison();
            let text = TerminalReporter::render_comparison_with_color(&comparison, false);
            assert!(text.contains("[UNCHANGED]       rpc-kept"), "{text}");
            assert!(text.contains("pass → pass"), "{text}");
        }

        #[test]
        fn the_header_reports_both_sides_and_the_summary_counts() {
            let comparison = regression_comparison();
            let text = TerminalReporter::render_comparison_with_color(&comparison, false);
            assert!(
                text.starts_with("Report comparison (diffVersion 1)"),
                "{text}"
            );
            assert!(
                text.contains("Baseline: 0.2.0, protocol 28, status pass"),
                "{text}"
            );
            assert!(
                text.contains("Current:  0.2.0, protocol 28, status fail"),
                "{text}"
            );
            assert!(
                text.contains("Checks: 1 regressed, 1 resolved, 0 added, 0 removed, 1 unchanged"),
                "{text}"
            );
        }

        #[test]
        fn lost_coverage_is_listed_after_the_checks() {
            // `rpc-kept` passes in the baseline and is absent right now: it
            // leaves the entries table as `removed` and gets its own list.
            let baseline = report(vec![result("rpc-kept", Surface::Rpc, Status::Pass)]);
            let current = report(vec![]);
            let comparison = compare(&baseline, &current, CompareOptions::default()).unwrap();
            let text = TerminalReporter::render_comparison_with_color(&comparison, false);
            assert!(text.contains("Missing checks (1):"), "{text}");
            assert!(
                text.contains("  rpc/rpc-kept passed in the baseline and is not running now"),
                "{text}"
            );
            assert!(text.contains("Verdict: [COVERAGE LOST]"), "{text}");
        }

        #[test]
        fn empty_sides_are_called_out_and_never_count_as_regressions() {
            let comparison =
                compare(&report(vec![]), &report(vec![]), CompareOptions::default()).unwrap();
            let text = TerminalReporter::render_comparison_with_color(&comparison, false);
            assert!(
                text.contains("Note: the baseline report is empty"),
                "{text}"
            );
            assert!(text.contains("Note: the current report is empty"), "{text}");
            assert!(text.contains("(no checks to compare)"), "{text}");
            assert!(!text.contains("[REGRESSED]"), "{text}");
        }

        #[test]
        fn an_observed_protocol_change_is_noted() {
            let mut baseline = report(vec![result("a", Surface::Rpc, Status::Pass)]);
            baseline.network = Some(crate::NetworkSummary {
                name: canary_core::NetworkName::Testnet,
                observed_protocol: Some(ProtocolVersion(28)),
                error: None,
            });
            let mut current = report(vec![result("a", Surface::Rpc, Status::Pass)]);
            current.network = Some(crate::NetworkSummary {
                name: canary_core::NetworkName::Testnet,
                observed_protocol: Some(ProtocolVersion(29)),
                error: None,
            });
            let comparison = compare(&baseline, &current, CompareOptions::default()).unwrap();
            let text = TerminalReporter::render_comparison_with_color(&comparison, false);
            assert!(
                text.contains("Note: the reports observed different live protocol versions."),
                "{text}"
            );
        }

        #[test]
        fn rendering_is_deterministic_for_one_comparison() {
            let comparison = regression_comparison();
            let once = TerminalReporter::render_comparison_with_color(&comparison, false);
            let twice = TerminalReporter::render_comparison_with_color(&comparison, false);
            assert_eq!(once, twice);
        }

        /// Pins the full badge vocabulary: every classification has exactly
        /// one badge spelling, so the tables stay aligned as a block.
        #[test]
        fn every_classification_has_a_flat_badge_spelling() {
            let expected = [
                (Classification::Unchanged, "[UNCHANGED]"),
                (Classification::Degraded, "[DEGRADED]"),
                (Classification::Regressed, "[REGRESSED]"),
                (Classification::Improved, "[IMPROVED]"),
                (Classification::Resolved, "[RESOLVED]"),
                (Classification::Added, "[ADDED]"),
                (Classification::AddedFailing, "[ADDED-FAILING]"),
                (Classification::Removed, "[REMOVED]"),
                (Classification::RemovedFailing, "[REMOVED-FAILING]"),
                (Classification::Skipped, "[SKIPPED]"),
                (Classification::SkippedFailing, "[SKIPPED-FAILING]"),
                (Classification::UnchangedSkip, "[UNCHANGED-SKIP]"),
            ];
            for (classification, badge) in expected {
                assert_eq!(classification_badge(classification), badge);
            }
        }

        /// The private helpers are exercised through the renderer above; this
        /// test only pins the width constant against every badge, so a future
        /// badge cannot silently outgrow the table column.
        #[test]
        fn the_badge_width_covers_the_widest_badge() {
            let badges = [
                Classification::Unchanged,
                Classification::Degraded,
                Classification::Regressed,
                Classification::Improved,
                Classification::Resolved,
                Classification::Added,
                Classification::AddedFailing,
                Classification::Removed,
                Classification::RemovedFailing,
                Classification::Skipped,
                Classification::SkippedFailing,
                Classification::UnchangedSkip,
            ];
            let widest =
                badges
                    .iter()
                    .map(|c| classification_badge(*c))
                    .fold(String::new(), |a, b| {
                        if b.len() > a.len() {
                            b.to_string()
                        } else {
                            a
                        }
                    });
            assert_eq!(classification_badge(Classification::RemovedFailing), widest);
            assert_eq!(widest.len(), BADGE_WIDTH, "{widest}");
        }
    }
}
