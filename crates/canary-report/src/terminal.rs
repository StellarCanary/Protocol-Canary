//! Default human-readable terminal output.

use std::fmt::Write as _;

use canary_core::{PolicyDecision, Status};

use crate::{ReportInput, SURFACE_ORDER};

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
    if input.has_any_error() {
        "ERROR"
    } else {
        match input.decision {
            PolicyDecision::Pass => "PASS",
            PolicyDecision::Warning => "WARNING",
            PolicyDecision::Fail => "NOT READY",
        }
    }
}

/// Renders [`ReportInput`] as the default terminal report.
pub struct TerminalReporter;

impl TerminalReporter {
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
                    let _ = writeln!(out, "Network: {} (protocol not observed)", network.name);
                }
            }
        }
        let _ = writeln!(out);

        for surface in SURFACE_ORDER {
            let results: Vec<_> = input.results_for(surface).collect();
            if results.is_empty() {
                continue;
            }
            let _ = writeln!(out, "{surface}");
            let all_passed = results.iter().all(|r| r.status == Status::Pass);
            if all_passed {
                let _ = writeln!(out, "  {}/{} PASS", results.len(), results.len());
            } else {
                for result in &results {
                    let _ = writeln!(
                        out,
                        "  {:<28} {}",
                        result.test_id,
                        status_badge(result.status)
                    );
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
    use crate::{ProjectSummary, ReportInput};
    use canary_core::{GitContext, ProjectType, ProtocolVersion};

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
            decision,
            git: GitContext::default(),
            verbose: false,
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
}
