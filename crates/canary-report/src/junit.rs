//! JUnit XML output, for CI test-summary dashboards.
//!
//! Renders the same normalized [`ReportInput`] every other reporter
//! consumes: results become `<testcase>` elements — `Fail` as `<failure>`,
//! `Error` as `<error>`, `Status::Skipped` results and planner-skipped
//! fixtures as `<skipped>` — grouped into one `<testsuite>` per surface,
//! under a single `<testsuites>` root carrying the aggregate counts.
//!
//! The XML is built with plain string formatting and explicit escaping
//! rather than an XML dependency: the element set is small and fixed, and
//! `CONTRIBUTING.md` asks for a reason before every new dependency. The
//! output is deterministic for a given input (no timestamps, no hostnames,
//! no floating point).

use std::fmt::Write as _;

use canary_core::Status;

use crate::{surface_heading, ReportInput, SURFACE_ORDER};

/// Renders a [`ReportInput`] as JUnit XML.
pub struct JUnitReporter;

/// Aggregates for one `<testsuite>` (and, summed, for `<testsuites>`).
///
/// `tests` counts every `<testcase>` — executed results *and* skipped
/// fixtures, since both appear as test cases — while `skipped` counts the
/// ones that did not run, matching how CI dashboards tally JUnit files.
#[derive(Debug, Default, Clone, Copy)]
struct Counts {
    tests: usize,
    failures: usize,
    errors: usize,
    skipped: usize,
    time_ms: u64,
}

/// How one result maps onto a JUnit outcome element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Pass,
    Failure,
    Error,
    Skipped,
}

impl Outcome {
    fn of(status: Status) -> Self {
        match status {
            // A warning is not a JUnit failure: the policy decision (and
            // the exit code) already decide whether it fails the run, and
            // JUnit has no warning state to express it with.
            Status::Pass | Status::Warning => Outcome::Pass,
            Status::Fail => Outcome::Failure,
            Status::Error => Outcome::Error,
            Status::Skipped => Outcome::Skipped,
        }
    }

    /// The outcome element inside the `<testcase>`, or an empty string for
    /// a passing case. `details` is the diagnostic body (the failure or
    /// error text); `summary` is the short message attribute and the
    /// fallback body when there are no details.
    fn element(self, summary: &str, details: Option<&str>) -> String {
        let body = details.unwrap_or(summary);
        match self {
            Outcome::Pass => String::new(),
            Outcome::Failure => format!(
                "<failure message=\"{}\" type=\"failure\">{}</failure>",
                escape_attribute(summary),
                escape_text(body)
            ),
            Outcome::Error => format!(
                "<error message=\"{}\" type=\"error\">{}</error>",
                escape_attribute(summary),
                escape_text(body)
            ),
            Outcome::Skipped => "<skipped/>".to_string(),
        }
    }
}

/// Seconds with millisecond precision, computed with integer arithmetic so
/// no floating point touches a reported value.
fn seconds(time_ms: u64) -> String {
    format!("{}.{:03}", time_ms / 1000, time_ms % 1000)
}

/// Escapes the five XML entities that may appear in element text.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escapes everything [`escape_text`] escapes plus the attribute-delimiter
/// quotes, so a value can never break out of its quotes.
fn escape_attribute(text: &str) -> String {
    let mut out = escape_text(text);
    out.reserve(text.len());
    out = out.replace('"', "&quot;").replace('\'', "&apos;");
    out
}

impl JUnitReporter {
    /// Serializes a report input into JUnit XML.
    ///
    /// One `<testsuite>` is emitted per surface that has results or
    /// skipped fixtures, in [`SURFACE_ORDER`]; a surface with neither is
    /// omitted entirely. Planner-skipped fixtures appear as skipped
    /// test cases in their surface's suite, with the skip reason as the
    /// `<skipped>` message.
    pub fn render(input: &ReportInput) -> String {
        let mut suites = String::new();
        let mut totals = Counts::default();

        for surface in SURFACE_ORDER {
            let results: Vec<_> = input.results_for(surface).collect();
            let skipped: Vec<_> = input
                .skipped
                .iter()
                .filter(|s| s.surface == surface)
                .collect();
            if results.is_empty() && skipped.is_empty() {
                continue;
            }

            let classname = format!("stellar-canary.{}", surface);
            let mut counts = Counts::default();
            let mut cases = String::new();

            for result in results {
                let outcome = Outcome::of(result.status);
                match outcome {
                    Outcome::Pass => {}
                    Outcome::Failure => counts.failures += 1,
                    Outcome::Error => counts.errors += 1,
                    Outcome::Skipped => counts.skipped += 1,
                }
                counts.tests += 1;
                counts.time_ms += result.duration_ms;

                let element = outcome.element(&result.summary, result.details.as_deref());
                let _ = writeln!(
                    cases,
                    "    <testcase classname=\"{}\" name=\"{}\" time=\"{}\">{}</testcase>",
                    escape_attribute(&classname),
                    escape_attribute(&result.test_id),
                    seconds(result.duration_ms),
                    element,
                );
            }

            for skip in &skipped {
                counts.tests += 1;
                counts.skipped += 1;
                let _ = writeln!(
                    cases,
                    "    <testcase classname=\"{}\" name=\"{}\" time=\"0.000\"><skipped message=\"{}\"/></testcase>",
                    escape_attribute(&classname),
                    escape_attribute(&skip.fixture_id),
                    escape_attribute(&skip.reason),
                );
            }

            let _ = writeln!(
                suites,
                "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{}\">",
                escape_attribute(surface_heading(surface)),
                counts.tests,
                counts.failures,
                counts.errors,
                counts.skipped,
                seconds(counts.time_ms),
            );
            suites.push_str(&cases);
            let _ = writeln!(suites, "  </testsuite>");

            totals.tests += counts.tests;
            totals.failures += counts.failures;
            totals.errors += counts.errors;
            totals.skipped += counts.skipped;
            totals.time_ms += counts.time_ms;
        }

        let mut out = String::new();
        let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        let _ = writeln!(
            out,
            "<testsuites name=\"stellar-canary\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{}\">",
            totals.tests,
            totals.failures,
            totals.errors,
            totals.skipped,
            seconds(totals.time_ms),
        );
        out.push_str(&suites);
        let _ = writeln!(out, "</testsuites>");
        out
    }
}

#[cfg(test)]
mod tests {
    use canary_core::{
        CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, Status,
        Surface,
    };

    use super::*;
    use crate::{ProjectSummary, SkipSummary};

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

    fn input(results: Vec<CompatibilityResult>, skipped: Vec<SkipSummary>) -> ReportInput {
        ReportInput {
            tool_version: "0.1.0".into(),
            target_protocol: ProtocolVersion(28),
            project: ProjectSummary {
                name: "example".into(),
                project_type: ProjectType::Soroban,
            },
            network: None,
            results,
            skipped,
            verification: Vec::new(),
            decision: PolicyDecision::Pass,
            git: GitContext::default(),
            verbose: false,
            run: None,
        }
    }

    #[test]
    fn renders_the_xml_declaration_and_testsuites_root() {
        let xml = JUnitReporter::render(&input(
            vec![result("p28-xdr-1", Surface::Xdr, Status::Pass)],
            Vec::new(),
        ));
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<testsuites name=\"stellar-canary\""));
        assert!(xml.trim_end().ends_with("</testsuites>"));
    }

    #[test]
    fn a_passing_result_is_an_empty_testcase() {
        let xml = JUnitReporter::render(&input(
            vec![result("p28-xdr-1", Surface::Xdr, Status::Pass)],
            Vec::new(),
        ));
        assert!(xml.contains(
            "<testcase classname=\"stellar-canary.xdr\" name=\"p28-xdr-1\" time=\"0.001\"></testcase>"
        ));
        assert!(!xml.contains("<failure"));
    }

    #[test]
    fn a_failure_carries_its_summary_and_details() {
        let mut failing = result("p28-xdr-1", Surface::Xdr, Status::Fail);
        failing.summary = "did not round-trip".into();
        failing.details = Some("input:  AAAA\noutput: AAAB".into());
        let xml = JUnitReporter::render(&input(vec![failing], Vec::new()));

        assert!(xml.contains("failures=\"1\""));
        assert!(xml.contains("<failure message=\"did not round-trip\" type=\"failure\">"));
        assert!(xml.contains("input:  AAAA\noutput: AAAB"));
    }

    #[test]
    fn an_error_maps_to_the_error_element() {
        let xml = JUnitReporter::render(&input(
            vec![result("p28-rpc-1", Surface::Rpc, Status::Error)],
            Vec::new(),
        ));
        assert!(xml.contains("errors=\"1\""));
        assert!(xml.contains("<error message=\"p28-rpc-1 summary\" type=\"error\">"));
    }

    #[test]
    fn a_skipped_fixture_becomes_a_skipped_testcase_with_its_reason() {
        let xml = JUnitReporter::render(&input(
            Vec::new(),
            vec![SkipSummary {
                fixture_id: "p28-soroban-1".into(),
                surface: Surface::Soroban,
                reason: "requires network access".into(),
                code: canary_core::SkipCode::Unknown,
            }],
        ));
        assert!(xml.contains("skipped=\"1\""));
        assert!(xml.contains(
            "<testcase classname=\"stellar-canary.soroban\" name=\"p28-soroban-1\" time=\"0.000\"><skipped message=\"requires network access\"/></testcase>"
        ));
    }

    /// Negative case from the issue: diagnostics may contain any of the
    /// XML metacharacters; they must be escaped, never emitted raw.
    #[test]
    fn special_characters_in_diagnostics_are_xml_escaped() {
        let mut failing = result("p28-xdr-1", Surface::Xdr, Status::Fail);
        failing.summary = "a<b>c & \"d\" 'e'".into();
        failing.details = Some("tag <script>alert('x')</script> & \"more\"".into());
        let xml = JUnitReporter::render(&input(
            vec![failing],
            vec![SkipSummary {
                fixture_id: "p28-rpc-1".into(),
                surface: Surface::Rpc,
                reason: "a < b & c > d".into(),
                code: canary_core::SkipCode::Unknown,
            }],
        ));

        assert!(!xml.contains("<script>"), "raw '<' leaked: {xml}");
        assert!(xml.contains("message=\"a&lt;b&gt;c &amp; &quot;d&quot; &apos;e&apos;\""));
        // Quotes need no escaping in element text — only in attributes —
        // so the body keeps them literal while `<`, `>` and `&` are escaped.
        assert!(xml.contains("tag &lt;script&gt;alert('x')&lt;/script&gt; &amp; \"more\""));
        assert!(xml.contains("message=\"a &lt; b &amp; c &gt; d\""));
    }

    #[test]
    fn output_is_deterministic_for_the_same_input() {
        let report = input(
            vec![
                result("p28-xdr-1", Surface::Xdr, Status::Pass),
                result("p28-xdr-2", Surface::Xdr, Status::Fail),
            ],
            vec![SkipSummary {
                fixture_id: "p28-rpc-1".into(),
                surface: Surface::Rpc,
                reason: "r".into(),
                code: canary_core::SkipCode::Unknown,
            }],
        );
        assert_eq!(
            JUnitReporter::render(&report),
            JUnitReporter::render(&report)
        );
    }
}
