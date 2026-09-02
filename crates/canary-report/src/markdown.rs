//! GitHub-friendly Markdown output, suitable for a job summary.

use std::fmt::Write as _;

use canary_core::Status;

use crate::{surface_heading, ReportInput, SURFACE_ORDER};

fn surface_cell(results: &[&canary_core::CompatibilityResult]) -> &'static str {
    if results.iter().any(|r| r.status == Status::Error) {
        "‼️ Error"
    } else if results.iter().any(|r| r.status == Status::Fail) {
        "❌ Fail"
    } else if results.iter().any(|r| r.status == Status::Warning) {
        "⚠️ Warning"
    } else {
        "✅ Pass"
    }
}

/// Renders a [`ReportInput`] as Markdown suitable for a GitHub job
/// summary.
pub struct MarkdownReporter;

impl MarkdownReporter {
    pub fn render(input: &ReportInput) -> String {
        let mut out = String::new();

        let _ = writeln!(out, "## Stellar Protocol Canary");
        let _ = writeln!(out);
        let _ = writeln!(out, "Protocol {} compatibility", input.target_protocol);
        let _ = writeln!(out);
        let _ = writeln!(out, "| Surface | Result |");
        let _ = writeln!(out, "|---|---|");

        for surface in SURFACE_ORDER {
            let results: Vec<_> = input.results_for(surface).collect();
            if results.is_empty() {
                continue;
            }
            let _ = writeln!(
                out,
                "| {} | {} |",
                surface_heading(surface),
                surface_cell(&results)
            );
        }
        let _ = writeln!(out);

        let _ = writeln!(
            out,
            "**Result: {}**",
            input.overall_status().as_str().to_uppercase()
        );

        let failures: Vec<_> = input
            .results
            .iter()
            .filter(|r| matches!(r.status, Status::Fail | Status::Error))
            .collect();
        if !failures.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "### Failures");
            for failure in failures {
                let _ = writeln!(out);
                let _ = writeln!(out, "- **{}**: {}", failure.test_id, failure.summary);
                if let Some(details) = &failure.details {
                    for line in details.lines() {
                        let _ = writeln!(out, "  - {line}");
                    }
                }
            }
        }

        if !input.skipped.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "Skipped {} fixture(s).", input.skipped.len());
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectSummary, ReportInput, SkipSummary};
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
        }
    }

    fn base_input(results: Vec<CompatibilityResult>, decision: PolicyDecision) -> ReportInput {
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
    fn renders_the_documented_table_shape_on_a_clean_pass() {
        let input = base_input(
            vec![
                result("p28-xdr-1", Surface::Xdr, Status::Pass),
                result("p28-rpc-1", Surface::Rpc, Status::Pass),
                result("p28-soroban-1", Surface::Soroban, Status::Pass),
            ],
            PolicyDecision::Pass,
        );
        let text = MarkdownReporter::render(&input);
        assert!(text.starts_with("## Stellar Protocol Canary"));
        assert!(text.contains("| Surface | Result |"));
        assert!(text.contains("| XDR | ✅ Pass |"));
        assert!(text.contains("| RPC | ✅ Pass |"));
        assert!(text.contains("| Soroban | ✅ Pass |"));
        assert!(text.contains("**Result: PASS**"));
        assert!(!text.contains("### Failures"));
    }

    #[test]
    fn a_failure_produces_a_failures_section_with_details() {
        let mut failing = result("p28-xdr-1", Surface::Xdr, Status::Fail);
        failing.details = Some("decode mismatch".to_string());
        let input = base_input(vec![failing], PolicyDecision::Fail);
        let text = MarkdownReporter::render(&input);
        assert!(text.contains("| XDR | ❌ Fail |"));
        assert!(text.contains("**Result: FAIL**"));
        assert!(text.contains("### Failures"));
        assert!(text.contains("p28-xdr-1"));
        assert!(text.contains("decode mismatch"));
    }

    #[test]
    fn an_execution_error_renders_as_error_result() {
        let input = base_input(
            vec![result("p28-rpc-1", Surface::Rpc, Status::Error)],
            PolicyDecision::Pass,
        );
        let text = MarkdownReporter::render(&input);
        assert!(text.contains("**Result: ERROR**"));
    }

    #[test]
    fn mentions_skipped_fixture_count_when_present() {
        let mut input = base_input(vec![], PolicyDecision::Pass);
        input.skipped.push(SkipSummary {
            fixture_id: "p28-soroban-1".into(),
            surface: Surface::Soroban,
            reason: "requires a capability not declared by this project".into(),
        });
        let text = MarkdownReporter::render(&input);
        assert!(text.contains("Skipped 1 fixture(s)."));
    }
}
