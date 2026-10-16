//! Comparing two reports (contract CF-05): result identity, the status
//! transition table, eligibility, and the deterministic `diffVersion` 1
//! JSON document.
//!
//! This module is a library only. It compares two [`ReportInput`] values (or
//! two JSON report texts through [`compare_json`]) and renders the result;
//! the `diff` subcommand, terminal and Markdown renderers are separate
//! units. Both inputs are untrusted files: sizes are capped, no path is
//! read or written, and strings copied into the output are report data that
//! whatever renders them must escape.
//!
//! # What this version does not compare
//!
//! The report model has no `fixturePack` digest and no skip `code` yet, so
//! `fixturePackChanged` is always `null` ("unknown, not false") and skip
//! reasons are not compared.

use std::collections::BTreeMap;

use canary_core::{ResultSource, Status};
use serde::Serialize;

use crate::json::{JsonReportError, JsonReporter};
use crate::ReportInput;

/// Version of the comparison document this module writes.
pub const DIFF_VERSION: u32 = 1;

/// Largest report text [`compare_json`] reads (decision D-06: 10 MiB).
pub const MAX_REPORT_BYTES: usize = 10 * 1024 * 1024;

/// The state of one identity in one report (CF-05 section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Pass,
    Warning,
    Fail,
    Error,
    Skipped,
    /// Not in `results` and not in `skipped`.
    Absent,
}

impl State {
    fn from_status(status: Status) -> State {
        match status {
            Status::Pass => State::Pass,
            Status::Warning => State::Warning,
            Status::Fail => State::Fail,
            Status::Error => State::Error,
            Status::Skipped => State::Skipped,
        }
    }

    fn is_good(self) -> bool {
        matches!(self, State::Pass | State::Warning)
    }

    fn is_failing(self) -> bool {
        matches!(self, State::Fail | State::Error)
    }
}

/// What a transition means (CF-05 section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Classification {
    Unchanged,
    Degraded,
    Regressed,
    Improved,
    Resolved,
    Added,
    AddedFailing,
    Removed,
    RemovedFailing,
    Skipped,
    SkippedFailing,
    UnchangedSkip,
}

/// The overall outcome of a comparison, evaluated in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Regression,
    CoverageLost,
    Improved,
    NoChange,
}

/// Classifies one transition and says whether it counts as a regression.
///
/// `warnings_are_failures` is the policy the *current* run was evaluated
/// under: with it, `pass` to `warning` is a regression. Every row of the
/// CF-05 section 4 table is covered by a golden test.
pub fn classify(from: State, to: State, warnings_are_failures: bool) -> (Classification, bool) {
    use Classification as C;
    use State::*;
    match (from, to) {
        (Absent, Absent) => (C::Unchanged, false),
        (a, b) if a == b => (C::Unchanged, false),
        (Pass, Warning) => (C::Degraded, warnings_are_failures),
        (Warning, Pass) => (C::Improved, false),
        (a, b) if a.is_good() && b.is_failing() => (C::Regressed, true),
        (Fail, Error) => (C::Degraded, true),
        (Error, Fail) => (C::Improved, false),
        (a, b) if a.is_failing() && b.is_good() => (C::Resolved, false),
        (Absent, Pass) | (Skipped, Pass | Warning) => (C::Added, false),
        (Absent | Skipped, b) if b.is_failing() => (C::AddedFailing, true),
        (Absent, Warning) => (C::AddedFailing, true),
        (a, Absent) if a.is_good() => (C::Removed, false),
        (a, Absent) if a.is_failing() => (C::RemovedFailing, false),
        (a, Skipped) if a.is_good() => (C::Skipped, false),
        (a, Skipped) if a.is_failing() => (C::SkippedFailing, true),
        (Skipped, Absent) | (Absent, Skipped) => (C::UnchangedSkip, false),
        // Every pair is handled above; kept total without a panic.
        _ => (C::Unchanged, false),
    }
}

/// Options for a comparison.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompareOptions {
    /// The current run treated warnings as failures (policy
    /// `warnings_are_failures`), which makes `pass` to `warning` a
    /// regression.
    pub warnings_are_failures: bool,
}

/// One side of a comparison, for [`Ineligible`] messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Baseline,
    Current,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Side::Baseline => "baseline",
            Side::Current => "current",
        })
    }
}

/// Why two reports cannot be compared. Each maps to exit code `2` in the
/// future `diff` command; no partial output is produced.
#[derive(Debug, thiserror::Error)]
pub enum Ineligible {
    #[error("the {side} report is not a valid report: {source}")]
    InvalidReport {
        side: Side,
        #[source]
        source: JsonReportError,
    },

    #[error("the {side} report is {size} bytes, over the {MAX_REPORT_BYTES}-byte limit")]
    TooLarge { side: Side, size: usize },

    #[error(
        "the {side} report has schemaVersion {found}, which this build does not support (expected {expected})"
    )]
    SchemaVersion {
        side: Side,
        found: u32,
        expected: u32,
    },

    #[error("the baseline targets protocol {baseline} and the current report protocol {current}; they are not comparable")]
    TargetProtocolDiffers { baseline: u32, current: u32 },

    #[error("the {side} report lists {surface}/{id:?} more than once")]
    DuplicateIdentity {
        side: Side,
        surface: String,
        id: String,
    },
}

/// Summary of one report, for the `baseline` and `current` keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportHeader {
    #[serde(rename = "toolVersion")]
    pub tool_version: String,
    #[serde(rename = "targetProtocol")]
    pub target_protocol: u32,
    pub status: String,
}

/// One identity whose state differs, or is carried over, between reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub surface: String,
    pub id: String,
    pub from: State,
    pub to: State,
    pub classification: Classification,
    pub regression: bool,
    /// Present only when a side's result was replayed from the cache: true
    /// when the two sides' sources differ.
    #[serde(rename = "sourceChanged", skip_serializing_if = "Option::is_none")]
    pub source_changed: Option<bool>,
}

/// An identity that passed in the baseline and is removed or skipped now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Missing {
    pub surface: String,
    pub id: String,
}

/// Counts over `entries` and `missing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Summary {
    /// Entries that count as regressions.
    pub regressed: usize,
    /// Entries classified `resolved`.
    pub resolved: usize,
    /// Entries classified `added` or `added-failing`.
    pub added: usize,
    /// Entries classified `removed` or `removed-failing`.
    pub removed: usize,
    /// Entries classified `unchanged` or `unchanged-skip`.
    pub unchanged: usize,
    pub missing: usize,
}

/// The `diffVersion` 1 document. Field order is the contract's key order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Comparison {
    #[serde(rename = "diffVersion")]
    pub diff_version: u32,
    pub comparable: bool,
    pub baseline: ReportHeader,
    pub current: ReportHeader,
    /// Always `None` until the report model carries `fixturePack`.
    #[serde(rename = "fixturePackChanged")]
    pub fixture_pack_changed: Option<bool>,
    /// True only when both reports observed a protocol and they differ.
    #[serde(rename = "observedProtocolChanged")]
    pub observed_protocol_changed: bool,
    #[serde(rename = "emptyBaseline")]
    pub empty_baseline: bool,
    #[serde(rename = "emptyCurrent")]
    pub empty_current: bool,
    pub entries: Vec<Entry>,
    pub missing: Vec<Missing>,
    pub summary: Summary,
    pub verdict: Verdict,
}

impl Comparison {
    /// The deterministic JSON text: pretty-printed, no timestamps, no
    /// paths, entries sorted by `(surface, id)` in UTF-8 byte order.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|e| format!("{{\"error\": \"failed to serialize comparison: {e}\"}}"))
    }
}

type Identity = (String, String);

/// Identity -> (state, source) for one report. Rejects duplicates.
fn index(
    report: &ReportInput,
    side: Side,
) -> Result<BTreeMap<Identity, (State, ResultSource)>, Ineligible> {
    let mut map = BTreeMap::new();
    let mut insert = |surface: String, id: String, state, source| {
        let key = (surface, id);
        if map.contains_key(&key) {
            return Err(Ineligible::DuplicateIdentity {
                side,
                surface: key.0,
                id: key.1,
            });
        }
        map.insert(key, (state, source));
        Ok(())
    };
    for result in &report.results {
        let id = result
            .fixture_id
            .clone()
            .unwrap_or_else(|| result.test_id.clone());
        insert(
            result.surface.to_string(),
            id,
            State::from_status(result.status),
            result.source,
        )?;
    }
    for skip in &report.skipped {
        insert(
            skip.surface.to_string(),
            skip.fixture_id.clone(),
            State::Skipped,
            ResultSource::Unknown,
        )?;
    }
    Ok(map)
}

/// Compares two parsed reports.
///
/// # Errors
///
/// [`Ineligible`] when the target protocols differ or either report
/// repeats an identity.
pub fn compare(
    baseline: &ReportInput,
    current: &ReportInput,
    options: CompareOptions,
) -> Result<Comparison, Ineligible> {
    if baseline.target_protocol != current.target_protocol {
        return Err(Ineligible::TargetProtocolDiffers {
            baseline: baseline.target_protocol.0,
            current: current.target_protocol.0,
        });
    }
    let before = index(baseline, Side::Baseline)?;
    let after = index(current, Side::Current)?;

    let empty_baseline = baseline.results.is_empty();
    let empty_current = current.results.is_empty();

    let identities: std::collections::BTreeSet<&Identity> =
        before.keys().chain(after.keys()).collect();

    let mut entries = Vec::with_capacity(identities.len());
    let mut missing = Vec::new();
    for identity in identities {
        let (from, from_source) = before
            .get(identity)
            .copied()
            .unwrap_or((State::Absent, ResultSource::Unknown));
        let (to, to_source) = after
            .get(identity)
            .copied()
            .unwrap_or((State::Absent, ResultSource::Unknown));
        let (classification, mut regression) = classify(from, to, options.warnings_are_failures);
        // An empty side cannot establish a regression.
        if empty_baseline || empty_current {
            regression = false;
        }
        if from.is_good() && matches!(to, State::Absent | State::Skipped) {
            missing.push(Missing {
                surface: identity.0.clone(),
                id: identity.1.clone(),
            });
        }
        let cached = from_source == ResultSource::Cache || to_source == ResultSource::Cache;
        entries.push(Entry {
            surface: identity.0.clone(),
            id: identity.1.clone(),
            from,
            to,
            classification,
            regression,
            source_changed: cached.then(|| from_source != to_source),
        });
    }

    let mut summary = Summary {
        missing: missing.len(),
        ..Summary::default()
    };
    for entry in &entries {
        use Classification as C;
        if entry.regression {
            summary.regressed += 1;
        }
        match entry.classification {
            C::Resolved => summary.resolved += 1,
            C::Added | C::AddedFailing => summary.added += 1,
            C::Removed | C::RemovedFailing => summary.removed += 1,
            C::Unchanged | C::UnchangedSkip => summary.unchanged += 1,
            _ => {}
        }
    }

    let verdict = if entries.iter().any(|e| e.regression) {
        Verdict::Regression
    } else if !missing.is_empty() {
        Verdict::CoverageLost
    } else if entries.iter().any(|e| {
        matches!(
            e.classification,
            Classification::Resolved | Classification::Improved | Classification::Added
        )
    }) {
        Verdict::Improved
    } else {
        Verdict::NoChange
    };

    let observed = |r: &ReportInput| r.network.as_ref().and_then(|n| n.observed_protocol);
    let observed_protocol_changed = match (observed(baseline), observed(current)) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    };

    Ok(Comparison {
        diff_version: DIFF_VERSION,
        comparable: true,
        baseline: header(baseline),
        current: header(current),
        fixture_pack_changed: None,
        observed_protocol_changed,
        empty_baseline,
        empty_current,
        entries,
        missing,
        summary,
        verdict,
    })
}

fn header(report: &ReportInput) -> ReportHeader {
    ReportHeader {
        tool_version: report.tool_version.clone(),
        target_protocol: report.target_protocol.0,
        status: report.overall_status().as_str().to_string(),
    }
}

/// Parses two JSON report texts (each at most [`MAX_REPORT_BYTES`]) and
/// compares them. Errors name which input is at fault.
pub fn compare_json(
    baseline: &str,
    current: &str,
    options: CompareOptions,
) -> Result<Comparison, Ineligible> {
    let baseline = parse_side(baseline, Side::Baseline)?;
    let current = parse_side(current, Side::Current)?;
    compare(&baseline, &current, options)
}

fn parse_side(text: &str, side: Side) -> Result<ReportInput, Ineligible> {
    if text.len() > MAX_REPORT_BYTES {
        return Err(Ineligible::TooLarge {
            side,
            size: text.len(),
        });
    }
    JsonReporter::parse(text).map_err(|source| match source {
        JsonReportError::UnsupportedSchemaVersion { found, expected } => {
            Ineligible::SchemaVersion {
                side,
                found,
                expected,
            }
        }
        source => Ineligible::InvalidReport { side, source },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectSummary, SkipSummary};
    use canary_core::{
        CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, Surface,
    };
    use State::*;

    fn result(id: &str, surface: Surface, status: Status) -> CompatibilityResult {
        CompatibilityResult {
            test_id: id.to_string(),
            protocol: ProtocolVersion(28),
            surface,
            status,
            summary: String::new(),
            details: None,
            duration_ms: 1,
            fixture_id: Some(id.to_string()),
            source: ResultSource::Live,
        }
    }

    fn report(results: Vec<CompatibilityResult>, skipped: Vec<SkipSummary>) -> ReportInput {
        ReportInput {
            tool_version: "0.1.1".into(),
            target_protocol: ProtocolVersion(28),
            project: ProjectSummary {
                name: "p".into(),
                project_type: ProjectType::Soroban,
            },
            network: None,
            results,
            skipped,
            decision: PolicyDecision::Pass,
            git: GitContext::default(),
            verbose: false,
            verification: Vec::new(),
            run: None,
        }
    }

    fn one(id: &str, status: Status) -> ReportInput {
        report(vec![result(id, Surface::Rpc, status)], Vec::new())
    }

    #[test]
    fn golden_transition_table_covers_every_row() {
        use Classification as C;
        let rows: &[(State, State, C, bool)] = &[
            (Pass, Pass, C::Unchanged, false),
            (Fail, Fail, C::Unchanged, false),
            (Pass, Warning, C::Degraded, false),
            (Pass, Fail, C::Regressed, true),
            (Pass, Error, C::Regressed, true),
            (Warning, Fail, C::Regressed, true),
            (Warning, Error, C::Regressed, true),
            (Fail, Error, C::Degraded, true),
            (Error, Fail, C::Improved, false),
            (Fail, Pass, C::Resolved, false),
            (Fail, Warning, C::Resolved, false),
            (Error, Pass, C::Resolved, false),
            (Error, Warning, C::Resolved, false),
            (Warning, Pass, C::Improved, false),
            (Absent, Pass, C::Added, false),
            (Absent, Warning, C::AddedFailing, true),
            (Absent, Fail, C::AddedFailing, true),
            (Absent, Error, C::AddedFailing, true),
            (Pass, Absent, C::Removed, false),
            (Warning, Absent, C::Removed, false),
            (Fail, Absent, C::RemovedFailing, false),
            (Error, Absent, C::RemovedFailing, false),
            (Pass, Skipped, C::Skipped, false),
            (Warning, Skipped, C::Skipped, false),
            (Fail, Skipped, C::SkippedFailing, true),
            (Error, Skipped, C::SkippedFailing, true),
            (Skipped, Pass, C::Added, false),
            (Skipped, Warning, C::Added, false),
            (Skipped, Fail, C::AddedFailing, true),
            (Skipped, Error, C::AddedFailing, true),
            (Skipped, Absent, C::UnchangedSkip, false),
            (Absent, Skipped, C::UnchangedSkip, false),
            (Absent, Absent, C::Unchanged, false),
        ];
        for &(from, to, class, regression) in rows {
            assert_eq!(
                classify(from, to, false),
                (class, regression),
                "{from:?} -> {to:?}"
            );
        }
    }

    #[test]
    fn warnings_policy_makes_pass_to_warning_a_regression() {
        assert_eq!(
            classify(Pass, Warning, true),
            (Classification::Degraded, true)
        );
    }

    #[test]
    fn classification_is_total() {
        let all = [Pass, Warning, Fail, Error, Skipped, Absent];
        for from in all {
            for to in all {
                let _ = classify(from, to, false);
            }
        }
    }

    #[test]
    fn contract_example_shape_and_key_order() {
        let comparison = compare(
            &one("example-rpc-fixture", Status::Pass),
            &one("example-rpc-fixture", Status::Fail),
            CompareOptions::default(),
        )
        .unwrap();
        assert_eq!(comparison.verdict, Verdict::Regression);
        let text = comparison.to_json();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["entries"][0]["classification"], "regressed");
        assert_eq!(value["summary"]["regressed"], 1);
        assert_eq!(value["fixturePackChanged"], serde_json::Value::Null);
        // Check the rendered text, which is what consumers diff.
        let order: Vec<usize> = [
            "\"diffVersion\"",
            "\"comparable\"",
            "\"baseline\"",
            "\"current\"",
            "\"fixturePackChanged\"",
            "\"observedProtocolChanged\"",
            "\"emptyBaseline\"",
            "\"emptyCurrent\"",
            "\"entries\"",
            "\"missing\"",
            "\"summary\"",
            "\"verdict\"",
        ]
        .iter()
        .map(|k| text.find(k).unwrap())
        .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
    }

    #[test]
    fn shuffling_results_gives_identical_bytes() {
        let ids = ["a", "b", "c", "d"];
        let mk = |order: &[usize], status: Status| {
            report(
                order
                    .iter()
                    .map(|&i| result(ids[i], Surface::Xdr, status))
                    .collect(),
                Vec::new(),
            )
        };
        let one = compare(
            &mk(&[0, 1, 2, 3], Status::Pass),
            &mk(&[3, 1, 0, 2], Status::Fail),
            CompareOptions::default(),
        )
        .unwrap()
        .to_json();
        let two = compare(
            &mk(&[2, 3, 1, 0], Status::Pass),
            &mk(&[0, 2, 3, 1], Status::Fail),
            CompareOptions::default(),
        )
        .unwrap()
        .to_json();
        assert_eq!(one, two);
    }

    #[test]
    fn entries_sort_by_surface_then_id_in_byte_order() {
        let current = report(
            vec![
                result("b", Surface::Xdr, Status::Pass),
                result("z", Surface::Rpc, Status::Pass),
                result("B", Surface::Rpc, Status::Pass),
            ],
            Vec::new(),
        );
        let comparison = compare(
            &report(Vec::new(), Vec::new()),
            &current,
            CompareOptions::default(),
        )
        .unwrap();
        let order: Vec<(&str, &str)> = comparison
            .entries
            .iter()
            .map(|e| (e.surface.as_str(), e.id.as_str()))
            .collect();
        assert_eq!(order, [("rpc", "B"), ("rpc", "z"), ("xdr", "b")]);
    }

    #[test]
    fn target_protocol_difference_is_refused() {
        let mut other = one("a", Status::Pass);
        other.target_protocol = ProtocolVersion(29);
        let err = compare(&one("a", Status::Pass), &other, CompareOptions::default()).unwrap_err();
        assert!(matches!(
            err,
            Ineligible::TargetProtocolDiffers {
                baseline: 28,
                current: 29
            }
        ));
    }

    #[test]
    fn duplicate_identities_are_rejected_on_either_side() {
        let dup = report(
            vec![
                result("a", Surface::Rpc, Status::Pass),
                result("a", Surface::Rpc, Status::Fail),
            ],
            Vec::new(),
        );
        let err = compare(&one("a", Status::Pass), &dup, CompareOptions::default()).unwrap_err();
        assert!(matches!(
            err,
            Ineligible::DuplicateIdentity {
                side: Side::Current,
                ..
            }
        ));
        // The same fixture in results and in skipped is also one identity twice.
        let both = report(
            vec![result("a", Surface::Rpc, Status::Pass)],
            vec![SkipSummary {
                fixture_id: "a".into(),
                surface: Surface::Rpc,
                reason: "r".into(),
            }],
        );
        assert!(compare(&both, &one("a", Status::Pass), CompareOptions::default()).is_err());
    }

    #[test]
    fn same_id_on_two_surfaces_is_two_identities() {
        let both = report(
            vec![
                result("a", Surface::Rpc, Status::Pass),
                result("a", Surface::Xdr, Status::Pass),
            ],
            Vec::new(),
        );
        assert_eq!(
            compare(&both, &both, CompareOptions::default())
                .unwrap()
                .entries
                .len(),
            2
        );
    }

    #[test]
    fn missing_and_verdict_order() {
        let baseline = report(
            vec![
                result("a", Surface::Rpc, Status::Pass),
                result("b", Surface::Rpc, Status::Pass),
            ],
            Vec::new(),
        );
        let current = report(
            vec![
                result("a", Surface::Rpc, Status::Pass),
                result("c", Surface::Rpc, Status::Pass),
            ],
            Vec::new(),
        );
        let cmp = compare(&baseline, &current, CompareOptions::default()).unwrap();
        assert_eq!(
            cmp.missing,
            [Missing {
                surface: "rpc".into(),
                id: "b".into()
            }]
        );
        // `c` is `added`, but lost coverage outranks improvement.
        assert_eq!(cmp.verdict, Verdict::CoverageLost);
        assert_eq!(cmp.summary.added, 1);
        assert_eq!(cmp.summary.removed, 1);
        assert_eq!(cmp.summary.missing, 1);
    }

    #[test]
    fn a_skipped_fixture_is_one_identity_with_two_states() {
        let skipped = report(
            Vec::new(),
            vec![SkipSummary {
                fixture_id: "a".into(),
                surface: Surface::Rpc,
                reason: "no network".into(),
            }],
        );
        let cmp = compare(&one("a", Status::Fail), &skipped, CompareOptions::default()).unwrap();
        assert_eq!(
            cmp.entries[0].classification,
            Classification::SkippedFailing
        );
        assert!(cmp.empty_current);
        // An empty side never reports a regression.
        assert!(!cmp.entries[0].regression);
    }

    #[test]
    fn improvement_and_no_change() {
        let cmp = compare(
            &one("a", Status::Fail),
            &one("a", Status::Pass),
            CompareOptions::default(),
        )
        .unwrap();
        assert_eq!(cmp.verdict, Verdict::Improved);
        assert_eq!(cmp.summary.resolved, 1);
        let same = compare(
            &one("a", Status::Pass),
            &one("a", Status::Pass),
            CompareOptions::default(),
        )
        .unwrap();
        assert_eq!(same.verdict, Verdict::NoChange);
        assert_eq!(same.summary.unchanged, 1);
    }

    #[test]
    fn cached_results_carry_source_changed() {
        let mut cached = one("a", Status::Pass);
        cached.results[0].source = ResultSource::Cache;
        let cmp = compare(&one("a", Status::Pass), &cached, CompareOptions::default()).unwrap();
        assert_eq!(cmp.entries[0].source_changed, Some(true));
        let plain = compare(
            &one("a", Status::Pass),
            &one("a", Status::Pass),
            CompareOptions::default(),
        )
        .unwrap();
        assert_eq!(plain.entries[0].source_changed, None);
    }

    #[test]
    fn json_inputs_name_the_side_at_fault() {
        let good = JsonReporter::render(&one("a", Status::Pass));
        let err = compare_json(&good, "{not json", CompareOptions::default()).unwrap_err();
        assert!(matches!(
            err,
            Ineligible::InvalidReport {
                side: Side::Current,
                ..
            }
        ));
        assert!(err.to_string().contains("current"));

        let v2 = good.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2");
        let err = compare_json(&v2, &good, CompareOptions::default()).unwrap_err();
        assert!(matches!(
            err,
            Ineligible::SchemaVersion {
                side: Side::Baseline,
                found: 2,
                ..
            }
        ));
    }

    #[test]
    fn oversized_reports_are_refused_before_parsing() {
        let big = " ".repeat(MAX_REPORT_BYTES + 1);
        let good = JsonReporter::render(&one("a", Status::Pass));
        let err = compare_json(&big, &good, CompareOptions::default()).unwrap_err();
        assert!(matches!(
            err,
            Ineligible::TooLarge {
                side: Side::Baseline,
                ..
            }
        ));
    }

    #[test]
    fn json_round_trip_comparison_matches_the_in_memory_one() {
        let a = one("a", Status::Pass);
        let b = one("a", Status::Fail);
        let direct = compare(&a, &b, CompareOptions::default()).unwrap();
        let via_json = compare_json(
            &JsonReporter::render(&a),
            &JsonReporter::render(&b),
            CompareOptions::default(),
        )
        .unwrap();
        assert_eq!(direct.entries, via_json.entries);
    }
}
