//! Default human-readable terminal output.

use std::fmt::Write as _;

use canary_core::Status;

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
        });
        let text = TerminalReporter::render(&input);
        assert!(text.contains("Skipped fixtures: 1"));
    }

    fn skip(fixture_id: &str, surface: canary_core::Surface, reason: &str) -> crate::SkipSummary {
        crate::SkipSummary {
            fixture_id: fixture_id.into(),
            surface,
            reason: reason.into(),
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
        ));
        input.skipped.push(skip(
            "p28-rpc-2",
            canary_core::Surface::Rpc,
            "no RPC endpoint configured",
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
        ));
        input.skipped.push(skip(
            "p28-rpc-2",
            canary_core::Surface::Rpc,
            "no RPC endpoint configured",
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
}
