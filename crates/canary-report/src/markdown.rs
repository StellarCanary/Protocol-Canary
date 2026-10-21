//! GitHub-friendly Markdown output, suitable for a job summary.

use std::fmt::Write as _;

use canary_core::Status;

use crate::diff::{Classification, Comparison, State, Verdict};
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
    /// Renders the given [`ReportInput`] into GitHub-friendly Markdown.
    ///
    /// The generated report includes:
    /// - Header with the target protocol version.
    /// - Summary table broken down by test surface (XDR, RPC, Soroban).
    /// - Overall status determination (**PASS**, **WARNING**, **FAIL**, or **ERROR**).
    /// - Detailed section for test failures with summary and details (if present).
    /// - Count of skipped fixtures (if any).
    ///
    /// # Arguments
    ///
    /// * `input` - Reference to the [`ReportInput`] containing test outcomes and policy decision.
    ///
    /// # Returns
    ///
    /// A formatted Markdown [`String`], ready to be rendered in a GitHub job summary or terminal log.
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
        if let Some(notice) = input.cache_notice() {
            let _ = writeln!(out);
            let _ = writeln!(out, "_{notice}_");
        }

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
            if input.verbose {
                for skip in &input.skipped {
                    let _ = writeln!(out, "- **{}**: {}", skip.fixture_id, skip.reason);
                }
            }
        }

        out
    }
}

/// The heading shown for one [`Verdict`]: the first line of the summary, so
/// a regression is visible before anything is expanded.
fn comparison_heading(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Regression => "## ❌ Report comparison: Regression",
        Verdict::CoverageLost => "## ⚠️ Report comparison: Coverage lost",
        Verdict::Improved => "## ✅ Report comparison: Improved",
        Verdict::NoChange => "## ✅ Report comparison: No change",
    }
}

/// The Markdown cell for one [`Classification`] in the outcome table.
///
/// Regression counts stay prominent: a regressed cell is bolded with a cross
/// mark, improvements get a check, and neutral transitions render explaining
/// text. Cell content is passed through [`comparison_cell`] before it is
/// written, so check ids from untrusted reports cannot break the table or
/// inject markup.
fn classification_cell(classification: Classification) -> String {
    match classification {
        Classification::Unchanged => "Unchanged".to_string(),
        Classification::Degraded => "⚠️ Degraded".to_string(),
        Classification::Regressed => "**❌ Regressed**".to_string(),
        Classification::Improved => "**✅ Improved**".to_string(),
        Classification::Resolved => "**✅ Resolved**".to_string(),
        Classification::Added => "**✅ Added**".to_string(),
        Classification::AddedFailing => "**❌ Added failing**".to_string(),
        Classification::Removed => "Removed".to_string(),
        Classification::RemovedFailing => "⚠️ Removed failing".to_string(),
        Classification::Skipped => "Skipped".to_string(),
        Classification::SkippedFailing => "**❌ Skipped failing**".to_string(),
        Classification::UnchangedSkip => "Unchanged (skip)".to_string(),
    }
}

/// The lowercase state name, matching `diffVersion` 1 storage (CF-05
/// section 5).
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

/// Escapes report-supplied text for one GitHub table cell.
///
/// GitHub Flavored Markdown reads a raw `|` as a column division and treats
/// line breaks as the end of the row, so both are neutralised: pipes are
/// backslash-escaped and carriage returns / line feeds collapse away.
/// Backslashes are escaped first, or the pipe escaping would double every
/// escape a report already carried.
fn comparison_cell(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            '\n' | '\r' => {}
            other => out.push(other),
        }
    }
    out
}

impl MarkdownReporter {
    /// Renders a report comparison (CF-05) as GitHub Flavored Markdown,
    /// suitable for a pull-request comment or a job step summary.
    ///
    /// The shape, in order: a verdict heading, a summary table whose
    /// regression row is bolded, the two reports' headers, a table of every
    /// changed check (with a collapsed `<details>` section for unchanged
    /// ones), and a missing-checks table when coverage was lost. Check ids
    /// come from untrusted report files, so every cell is passed through
    /// [`comparison_cell`]. The output is deterministic for a given
    /// [`Comparison`] — no timestamps, no paths.
    ///
    /// # Examples
    ///
    /// ```text
    /// let text = MarkdownReporter::render_comparison(&comparison);
    /// println!("{text}");
    /// ```
    pub fn render_comparison(comparison: &Comparison) -> String {
        let mut out = String::new();
        let summary = &comparison.summary;

        let _ = writeln!(out, "{}", comparison_heading(comparison.verdict));
        let _ = writeln!(out);

        // The summary table is the first thing a reviewer reads; the
        // regression row is bolded there so a non-zero count cannot be
        // skimmed past.
        let _ = writeln!(out, "| Outcome | Checks |");
        let _ = writeln!(out, "|---|---:|");
        if summary.regressed > 0 {
            let _ = writeln!(out, "| **Regressed** | **{}** |", summary.regressed);
        } else {
            let _ = writeln!(out, "| Regressed | 0 |");
        }
        let _ = writeln!(out, "| Resolved | {} |", summary.resolved);
        let _ = writeln!(out, "| Added | {} |", summary.added);
        let _ = writeln!(out, "| Removed | {} |", summary.removed);
        let _ = writeln!(out, "| Unchanged | {} |", summary.unchanged);
        let _ = writeln!(out, "| Coverage lost | {} |", summary.missing);
        let _ = writeln!(out);

        let _ = writeln!(out, "| | Tool version | Protocol | Status |");
        let _ = writeln!(out, "|---|---|---|---|");
        let _ = writeln!(
            out,
            "| Baseline | {} | {} | {} |",
            comparison_cell(&comparison.baseline.tool_version),
            comparison.baseline.target_protocol,
            comparison_cell(&comparison.baseline.status)
        );
        let _ = writeln!(
            out,
            "| Current | {} | {} | {} |",
            comparison_cell(&comparison.current.tool_version),
            comparison.current.target_protocol,
            comparison_cell(&comparison.current.status)
        );
        let _ = writeln!(out);

        if comparison.observed_protocol_changed {
            let _ = writeln!(
                out,
                "_The two reports observed different live protocol versions._"
            );
            let _ = writeln!(out);
        }
        if comparison.empty_baseline {
            let _ = writeln!(
                out,
                "_The baseline report is empty, so no change can be attributed to it._"
            );
            let _ = writeln!(out);
        }
        if comparison.empty_current {
            let _ = writeln!(
                out,
                "_The current report is empty, so no result can be judged against it._"
            );
            let _ = writeln!(out);
        }

        let mut changed = Vec::new();
        let mut unchanged = Vec::new();
        for entry in &comparison.entries {
            if matches!(
                entry.classification,
                Classification::Unchanged | Classification::UnchangedSkip
            ) {
                unchanged.push(entry);
            } else {
                changed.push(entry);
            }
        }

        if !changed.is_empty() {
            let _ = writeln!(out, "| Surface | Check | Was | Now | Outcome |");
            let _ = writeln!(out, "|---|---|---|---|---|");
            for entry in &changed {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} | {} |",
                    comparison_cell(&entry.surface),
                    comparison_cell(&entry.id),
                    state_label(entry.from),
                    state_label(entry.to),
                    classification_cell(entry.classification)
                );
            }
            let _ = writeln!(out);
        }

        if !unchanged.is_empty() {
            let _ = writeln!(out, "<details>");
            let _ = writeln!(
                out,
                "<summary>{} check(s) unchanged</summary>",
                unchanged.len()
            );
            let _ = writeln!(out);
            let _ = writeln!(out, "| Surface | Check | Was | Now |");
            let _ = writeln!(out, "|---|---|---|---|");
            for entry in &unchanged {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} |",
                    comparison_cell(&entry.surface),
                    comparison_cell(&entry.id),
                    state_label(entry.from),
                    state_label(entry.to)
                );
            }
            let _ = writeln!(out, "</details>");
            let _ = writeln!(out);
        }

        if !comparison.missing.is_empty() {
            let _ = writeln!(out, "### Lost coverage");
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "These checks passed in the baseline and do not run in the current report:"
            );
            let _ = writeln!(out);
            let _ = writeln!(out, "| Surface | Check |");
            let _ = writeln!(out, "|---|---|");
            for missing in &comparison.missing {
                let _ = writeln!(
                    out,
                    "| {} | {} |",
                    comparison_cell(&missing.surface),
                    comparison_cell(&missing.id)
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
            source: canary_core::ResultSource::Live,
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
            verification: vec![],
            decision,
            git: GitContext::default(),
            verbose: false,
            run: None,
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
    fn a_failure_with_multiline_details_renders_as_nested_bullets() {
        let mut failing = result("p28-xdr-2", Surface::Xdr, Status::Fail);
        failing.details =
            Some("error summary\nbyte offset: 12\nexpected something else".to_string());
        let input = base_input(vec![failing], PolicyDecision::Fail);
        let text = MarkdownReporter::render(&input);
        let expected = "- **p28-xdr-2**: p28-xdr-2 summary\n  - error summary\n  - byte offset: 12\n  - expected something else\n";
        assert!(text.contains(expected));
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
            code: canary_core::SkipCode::MissingCapability,
        });
        let text = MarkdownReporter::render(&input);
        assert!(text.contains("Skipped 1 fixture(s)."));
        assert!(!text.contains("requires a capability"));
    }

    #[test]
    fn mentions_skipped_fixture_reasons_when_verbose() {
        let mut input = base_input(vec![], PolicyDecision::Pass);
        input.verbose = true;
        input.skipped.push(SkipSummary {
            fixture_id: "p28-soroban-1".into(),
            surface: Surface::Soroban,
            reason: "requires a capability not declared by this project".into(),
            code: canary_core::SkipCode::MissingCapability,
        });
        let text = MarkdownReporter::render(&input);
        assert!(text.contains("Skipped 1 fixture(s)."));
        assert!(text
            .contains("- **p28-soroban-1**: requires a capability not declared by this project"));
    }

    /// The mirror of the test above: `render` guards the line with
    /// `if !input.skipped.is_empty()`, and every other test in this module
    /// happens to use an empty skipped list — so the omission was only ever
    /// an accidental side effect, never asserted. A regression that printed
    /// "Skipped 0 fixture(s)." on every clean run would have passed the
    /// whole suite.
    #[test]
    fn omits_the_skipped_fixture_line_when_nothing_was_skipped() {
        let input = base_input(
            vec![result("p28-xdr-1", Surface::Xdr, Status::Pass)],
            PolicyDecision::Pass,
        );
        assert!(input.skipped.is_empty());

        let text = MarkdownReporter::render(&input);
        assert!(
            !text.contains("Skipped"),
            "a run with no skipped fixtures must not mention them at all; \
             rendered report:\n{text}"
        );
        // Sanity: the report itself still rendered.
        assert!(text.starts_with("## Stellar Protocol Canary"));
        assert!(text.contains("**Result: PASS**"));
    }

    mod comparison {
        use super::*;
        use crate::diff::{compare, CompareOptions};
        use crate::NetworkSummary;
        use canary_core::{
            CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, Surface,
        };

        fn result(id: &str, surface: Surface, status: Status) -> CompatibilityResult {
            CompatibilityResult {
                test_id: id.to_string(),
                protocol: ProtocolVersion(28),
                surface,
                status,
                summary: format!("{id} summary"),
                details: None,
                duration_ms: 1,
                fixture_id: Some(id.to_string()),
                source: canary_core::ResultSource::Live,
            }
        }

        fn report(results: Vec<CompatibilityResult>) -> crate::ReportInput {
            crate::ReportInput {
                tool_version: "0.2.0".into(),
                target_protocol: ProtocolVersion(28),
                project: crate::ProjectSummary {
                    name: "example".into(),
                    project_type: ProjectType::Soroban,
                },
                network: None,
                results,
                skipped: Vec::new(),
                verification: Vec::new(),
                decision: PolicyDecision::Pass,
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
            let current = report(vec![
                result("rpc-kept", Surface::Rpc, Status::Pass),
                result("rpc-broke", Surface::Rpc, Status::Fail),
                result("rpc-fixed", Surface::Rpc, Status::Pass),
            ]);
            compare(&baseline, &current, CompareOptions::default()).unwrap()
        }

        #[test]
        fn the_verdict_heading_names_each_outcome() {
            let cases = [
                (
                    regression_comparison(),
                    "## ❌ Report comparison: Regression",
                ),
                (
                    {
                        let baseline = report(vec![result("a", Surface::Rpc, Status::Fail)]);
                        let current = report(vec![result("a", Surface::Rpc, Status::Pass)]);
                        compare(&baseline, &current, CompareOptions::default()).unwrap()
                    },
                    "## ✅ Report comparison: Improved",
                ),
            ];
            for (comparison, heading) in cases {
                let text = MarkdownReporter::render_comparison(&comparison);
                assert!(text.starts_with(heading), "{text}");
            }

            let baseline = report(vec![result("a", Surface::Rpc, Status::Pass)]);
            let current = report(vec![]);
            let lost = compare(&baseline, &current, CompareOptions::default()).unwrap();
            let text = MarkdownReporter::render_comparison(&lost);
            assert!(
                text.starts_with("## ⚠️ Report comparison: Coverage lost"),
                "{text}"
            );

            let text = MarkdownReporter::render_comparison(
                &compare(
                    &report(vec![result("a", Surface::Rpc, Status::Pass)]),
                    &report(vec![result("a", Surface::Rpc, Status::Pass)]),
                    CompareOptions::default(),
                )
                .unwrap(),
            );
            assert!(
                text.starts_with("## ✅ Report comparison: No change"),
                "{text}"
            );
        }

        #[test]
        fn the_summary_table_bolds_a_nonzero_regression_count() {
            let text = MarkdownReporter::render_comparison(&regression_comparison());
            assert!(text.contains("| **Regressed** | **1** |"), "{text}");

            let text = MarkdownReporter::render_comparison(
                &compare(
                    &report(vec![result("a", Surface::Rpc, Status::Pass)]),
                    &report(vec![result("a", Surface::Rpc, Status::Pass)]),
                    CompareOptions::default(),
                )
                .unwrap(),
            );
            assert!(text.contains("| Regressed | 0 |"), "{text}");
        }

        #[test]
        fn changed_checks_render_as_a_table_and_improvements_are_marked() {
            let text = MarkdownReporter::render_comparison(&regression_comparison());
            assert!(
                text.contains("| Surface | Check | Was | Now | Outcome |"),
                "{text}"
            );
            assert!(
                text.contains("| rpc | rpc-broke | pass | fail | **❌ Regressed** |"),
                "{text}"
            );
            assert!(
                text.contains("| rpc | rpc-fixed | fail | pass | **✅ Resolved** |"),
                "{text}"
            );
        }

        #[test]
        fn unchanged_checks_collapse_into_details() {
            let text = MarkdownReporter::render_comparison(&regression_comparison());
            assert!(text.contains("<details>"), "{text}");
            assert!(
                text.contains("<summary>1 check(s) unchanged</summary>"),
                "{text}"
            );
            assert!(text.contains("| rpc | rpc-kept | pass | pass |"), "{text}");
            assert!(text.contains("</details>"), "{text}");
        }

        #[test]
        fn coverage_lost_lists_the_missing_checks() {
            let baseline = report(vec![
                result("rpc-kept", Surface::Rpc, Status::Pass),
                result("rpc-broke", Surface::Rpc, Status::Pass),
            ]);
            let current = report(vec![result("rpc-broke", Surface::Rpc, Status::Fail)]);
            let comparison = compare(&baseline, &current, CompareOptions::default()).unwrap();
            let text = MarkdownReporter::render_comparison(&comparison);
            assert!(text.contains("### Lost coverage"), "{text}");
            assert!(text.contains("| rpc | rpc-kept |"), "{text}");
            assert!(text.contains("| **Regressed** |"), "{text}");
        }

        #[test]
        fn table_cells_escape_pipes_and_line_breaks() {
            let baseline = report(vec![result("a|pipe\nnewline", Surface::Rpc, Status::Pass)]);
            let current = report(vec![result("normal", Surface::Rpc, Status::Pass)]);
            let comparison = compare(&baseline, &current, CompareOptions::default()).unwrap();
            let text = MarkdownReporter::render_comparison(&comparison);
            // The escaped id stays inside one cell of one row: the `|` is
            // backslash-escaped and the line break collapses away, so the row
            // keeps its five columns.
            assert!(
                text.contains("| rpc | a\\|pipenewline | pass | absent | Removed |"),
                "{text}"
            );
        }

        #[test]
        fn the_sides_summary_and_observational_notes_render() {
            let mut baseline = report(vec![result("a", Surface::Rpc, Status::Pass)]);
            baseline.network = Some(NetworkSummary {
                name: canary_core::NetworkName::Testnet,
                observed_protocol: Some(ProtocolVersion(28)),
                error: None,
            });
            let mut current = report(vec![result("a", Surface::Rpc, Status::Pass)]);
            current.network = Some(NetworkSummary {
                name: canary_core::NetworkName::Testnet,
                observed_protocol: Some(ProtocolVersion(29)),
                error: None,
            });
            let comparison = compare(&baseline, &current, CompareOptions::default()).unwrap();
            let text = MarkdownReporter::render_comparison(&comparison);
            assert!(text.contains("| Baseline | 0.2.0 | 28 | pass |"), "{text}");
            assert!(text.contains("| Current | 0.2.0 | 28 | pass |"), "{text}");
            assert!(
                text.contains("_The two reports observed different live protocol versions._"),
                "{text}"
            );
        }

        #[test]
        fn empty_sides_are_noted_and_never_report_regressions() {
            let comparison =
                compare(&report(vec![]), &report(vec![]), CompareOptions::default()).unwrap();
            let text = MarkdownReporter::render_comparison(&comparison);
            assert!(text.contains("_The baseline report is empty"), "{text}");
            assert!(text.contains("_The current report is empty"), "{text}");
            assert!(text.contains("| Regressed | 0 |"), "{text}");
            assert!(!text.contains("**❌ Regressed**"), "{text}");
        }

        #[test]
        fn rendering_is_deterministic_for_one_comparison() {
            let comparison = regression_comparison();
            assert_eq!(
                MarkdownReporter::render_comparison(&comparison),
                MarkdownReporter::render_comparison(&comparison)
            );
        }
    }
}
