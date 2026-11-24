//! Report-parsing identity tests (issue #475): a report whose `results`
//! repeat a `(surface, id)` identity is rejected per CF-01 section 4,
//! while unique reports — including the same id on two surfaces, and a
//! skipped entry sharing a result's identity — still parse.

use canary_core::{
    CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, ResultSource,
    Status, Surface,
};
use canary_report::json::JsonReportError;
use canary_report::{JsonReporter, ProjectSummary, ReportInput, SkipSummary};

fn result(test_id: &str, surface: Surface) -> CompatibilityResult {
    CompatibilityResult {
        test_id: test_id.to_string(),
        protocol: ProtocolVersion(28),
        surface,
        status: Status::Pass,
        summary: "decoded successfully".to_string(),
        details: None,
        duration_ms: 1,
        fixture_id: Some(test_id.to_string()),
        source: ResultSource::Live,
    }
}

fn report_with(results: Vec<CompatibilityResult>, skipped: Vec<SkipSummary>) -> ReportInput {
    ReportInput {
        tool_version: "0.2.0".into(),
        target_protocol: ProtocolVersion(28),
        project: ProjectSummary {
            name: "example".into(),
            project_type: ProjectType::GenericStellar,
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

fn render_then_parse(input: &ReportInput) -> Result<ReportInput, JsonReportError> {
    JsonReporter::parse(&JsonReporter::render(input))
}

#[test]
fn rejects_a_report_with_a_duplicate_surface_and_id() {
    let input = report_with(
        vec![
            result("p28-xdr-1", Surface::Xdr),
            result("p28-rpc-1", Surface::Rpc),
            // The duplicate repeats the first result's identity: the report
            // must be rejected rather than silently keeping one entry.
            result("p28-xdr-1", Surface::Xdr),
        ],
        vec![],
    );

    let err = render_then_parse(&input).expect_err("duplicate identity must be rejected");
    match &err {
        JsonReportError::DuplicateResultIdentity { surface, id } => {
            assert_eq!(surface, "xdr");
            assert_eq!(id, "p28-xdr-1");
        }
        other => panic!("expected DuplicateResultIdentity, got {other:?}"),
    }

    // The diagnostic names the conflicting surface and fixture id.
    let message = err.to_string();
    assert!(message.contains("xdr"), "{message}");
    assert!(message.contains("p28-xdr-1"), "{message}");
}

#[test]
fn rejects_a_duplicate_even_when_only_the_last_of_many_entries_conflicts() {
    let input = report_with(
        vec![
            result("p28-xdr-1", Surface::Xdr),
            result("p28-rpc-1", Surface::Rpc),
            result("p28-soroban-1", Surface::Soroban),
            result("p28-rpc-1", Surface::Rpc),
        ],
        vec![],
    );

    let err = render_then_parse(&input).expect_err("duplicate identity must be rejected");
    assert!(matches!(
        err,
        JsonReportError::DuplicateResultIdentity { ref surface, ref id }
            if surface == "rpc" && id == "p28-rpc-1"
    ));
}

#[test]
fn accepts_the_same_id_on_different_surfaces() {
    // The identity is the (surface, id) *pair*: the same fixture id checked
    // through two surfaces is legitimate.
    let input = report_with(
        vec![
            result("p28-shared-1", Surface::Xdr),
            result("p28-shared-1", Surface::Rpc),
        ],
        vec![],
    );

    let parsed = render_then_parse(&input).expect("distinct surfaces are not duplicates");
    assert_eq!(parsed.results.len(), 2);
    assert_eq!(parsed.results[0].test_id, "p28-shared-1");
    assert_eq!(parsed.results[1].test_id, "p28-shared-1");
}

#[test]
fn accepts_a_report_where_every_identity_is_unique() {
    let input = report_with(
        vec![
            result("p28-xdr-1", Surface::Xdr),
            result("p28-xdr-2", Surface::Xdr),
            result("p28-rpc-1", Surface::Rpc),
            result("p28-soroban-1", Surface::Soroban),
        ],
        vec![],
    );

    let parsed = render_then_parse(&input).expect("unique identities parse");
    assert_eq!(parsed.results.len(), 4);
}

#[test]
fn a_skipped_entry_may_share_a_result_identity() {
    // CF-01's own example skips a fixture under the same id a result uses;
    // the uniqueness rule applies to `results`, not to the skip list.
    let input = report_with(
        vec![result("example-xdr-fixture", Surface::Xdr)],
        vec![SkipSummary {
            fixture_id: "example-xdr-fixture".to_string(),
            surface: Surface::Xdr,
            reason: "fixture targets protocol 28, this run targets protocol 29".to_string(),
            code: canary_core::SkipCode::UnsupportedProtocol,
        }],
    );

    let parsed = render_then_parse(&input).expect("skip list is not part of the identity rule");
    assert_eq!(parsed.results.len(), 1);
    assert_eq!(parsed.skipped.len(), 1);
}

// ---------------------------------------------------------------------------
// Duplicate identities during report aggregation (issue #744)
// ---------------------------------------------------------------------------
//
// Two layers guard identity uniqueness. `JsonReporter::parse` rejects a
// document whose `results` repeat a `(surface, testId)` pair (CF-01 section
// 4). `diff::compare`, which aggregates two reports by identity, indexes
// *every* entry — results by `fixtureId` (falling back to `testId`) and the
// skip list by `fixtureId` — and refuses a side that repeats one (CF-05
// section 7). The tests below pin both layers, the diagnostics they produce,
// and that the totals of an accepted report count each distinct check once.

use canary_report::diff::{compare, Classification, CompareOptions, Ineligible, Side, Verdict};
use serde_json::Value;

fn result_with(test_id: &str, surface: Surface, status: Status) -> CompatibilityResult {
    CompatibilityResult {
        status,
        ..result(test_id, surface)
    }
}

fn skip(fixture_id: &str, surface: Surface) -> SkipSummary {
    SkipSummary {
        fixture_id: fixture_id.to_string(),
        surface,
        reason: "fixture targets protocol 28, this run targets protocol 29".to_string(),
        code: canary_core::SkipCode::UnsupportedProtocol,
    }
}

#[test]
fn a_duplicate_with_conflicting_outcomes_is_rejected_not_resolved() {
    // A pass and a fail for one identity: keeping either would silently
    // change the report's verdict, so neither may win.
    for (first, second) in [(Status::Pass, Status::Fail), (Status::Fail, Status::Pass)] {
        let input = report_with(
            vec![
                result_with("p28-xdr-1", Surface::Xdr, first),
                result_with("p28-xdr-1", Surface::Xdr, second),
            ],
            vec![],
        );
        let err = render_then_parse(&input)
            .expect_err("a conflicting duplicate must be rejected whatever its order");
        assert!(
            matches!(
                &err,
                JsonReportError::DuplicateResultIdentity { surface, id }
                    if surface == "xdr" && id == "p28-xdr-1"
            ),
            "{first:?} then {second:?}: {err:?}"
        );
    }
}

#[test]
fn the_parse_diagnostic_is_descriptive_enough_to_act_on() {
    let input = report_with(
        vec![
            result("p28-soroban-7", Surface::Soroban),
            result("p28-soroban-7", Surface::Soroban),
        ],
        vec![],
    );
    let message = render_then_parse(&input)
        .expect_err("duplicate identity must be rejected")
        .to_string();
    assert_eq!(
        message,
        "duplicate result identity in JSON report: surface \"soroban\", id \"p28-soroban-7\"",
        "the message names the rule, the surface and the id"
    );
}

#[test]
fn aggregation_rejects_a_side_that_repeats_an_identity_and_names_it() {
    let clean = report_with(vec![result("p28-rpc-1", Surface::Rpc)], vec![]);
    let duplicated = report_with(
        vec![
            result("p28-rpc-1", Surface::Rpc),
            result("p28-rpc-1", Surface::Rpc),
        ],
        vec![],
    );

    for (baseline, current, side) in [
        (&duplicated, &clean, Side::Baseline),
        (&clean, &duplicated, Side::Current),
    ] {
        let err = compare(baseline, current, CompareOptions::default())
            .expect_err("a repeated identity makes the pair ineligible");
        match &err {
            Ineligible::DuplicateIdentity {
                side: at_fault,
                surface,
                id,
            } => {
                assert_eq!(*at_fault, side);
                assert_eq!(surface, "rpc");
                assert_eq!(id, "p28-rpc-1");
            }
            other => panic!("expected DuplicateIdentity, got {other}"),
        }
        assert_eq!(
            err.to_string(),
            format!("the {side} report lists rpc/\"p28-rpc-1\" more than once"),
            "the message names the side, the surface and the id"
        );
    }
}

#[test]
fn aggregation_checks_the_baseline_before_the_current_report() {
    // Both sides are corrupt; the refusal is deterministic and names the
    // baseline, the first input the comparison reads.
    let duplicated = report_with(
        vec![
            result("p28-xdr-1", Surface::Xdr),
            result("p28-xdr-1", Surface::Xdr),
        ],
        vec![],
    );
    let err = compare(&duplicated, &duplicated, CompareOptions::default()).unwrap_err();
    assert!(
        matches!(
            err,
            Ineligible::DuplicateIdentity {
                side: Side::Baseline,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn aggregation_identifies_results_by_fixture_id_not_test_id() {
    // Two results with distinct test ids that ran the same fixture on the
    // same surface: the JSON layer (keyed on testId) accepts the document,
    // but aggregation keys on fixtureId, so the comparison refuses it rather
    // than collapsing two outcomes into one entry.
    let mut first = result("p28-xdr-run-a", Surface::Xdr);
    first.fixture_id = Some("p28-xdr-shared".to_string());
    let mut second = result_with("p28-xdr-run-b", Surface::Xdr, Status::Fail);
    second.fixture_id = Some("p28-xdr-shared".to_string());
    let input = report_with(vec![first, second], vec![]);

    let parsed = render_then_parse(&input).expect("distinct testIds pass the JSON identity rule");
    assert_eq!(parsed.results.len(), 2);

    let err = compare(&parsed, &parsed, CompareOptions::default())
        .expect_err("a shared fixtureId is one identity to the comparison");
    assert!(
        matches!(
            &err,
            Ineligible::DuplicateIdentity { surface, id, .. }
                if surface == "xdr" && id == "p28-xdr-shared"
        ),
        "{err}"
    );
}

#[test]
fn a_skip_sharing_a_result_identity_parses_but_cannot_be_aggregated() {
    // CF-01 permits the shape (the uniqueness rule covers `results` only), but
    // a comparison cannot give one identity two states, so CF-05 refuses it.
    let input = report_with(
        vec![result("example-xdr-fixture", Surface::Xdr)],
        vec![skip("example-xdr-fixture", Surface::Xdr)],
    );
    let parsed = render_then_parse(&input).expect("the skip list is outside the CF-01 rule");

    let err = compare(&parsed, &parsed, CompareOptions::default())
        .expect_err("one identity cannot be both a result and a skip");
    assert!(
        matches!(
            &err,
            Ineligible::DuplicateIdentity { surface, id, .. }
                if surface == "xdr" && id == "example-xdr-fixture"
        ),
        "{err}"
    );
}

#[test]
fn a_skip_list_that_repeats_an_identity_cannot_be_aggregated() {
    let input = report_with(
        vec![result("p28-rpc-1", Surface::Rpc)],
        vec![
            skip("p28-soroban-1", Surface::Soroban),
            skip("p28-soroban-1", Surface::Soroban),
        ],
    );
    let err = compare(&input, &input, CompareOptions::default())
        .expect_err("a repeated skip is a repeated identity");
    assert!(
        matches!(
            &err,
            Ineligible::DuplicateIdentity { surface, id, .. }
                if surface == "soroban" && id == "p28-soroban-1"
        ),
        "{err}"
    );
}

#[test]
fn totals_count_each_distinct_identity_exactly_once() {
    // Five distinct identities — the same fixture id on two surfaces is two
    // of them — with a mix of outcomes, plus one skip on its own identity.
    let input = report_with(
        vec![
            result_with("p28-shared-1", Surface::Xdr, Status::Pass),
            result_with("p28-shared-1", Surface::Rpc, Status::Fail),
            result_with("p28-xdr-2", Surface::Xdr, Status::Warning),
            result_with("p28-soroban-1", Surface::Soroban, Status::Error),
            result_with("p28-rpc-2", Surface::Rpc, Status::Pass),
        ],
        vec![skip("p28-soroban-2", Surface::Soroban)],
    );

    // The rendered `counts` block: `total` is the distinct results, the
    // per-outcome counts partition it, and skips are counted apart.
    let rendered: Value =
        serde_json::from_str(&JsonReporter::render(&input)).expect("the report is JSON");
    let counts = &rendered["counts"];
    assert_eq!(counts["total"], 5);
    assert_eq!(counts["passed"], 2);
    assert_eq!(counts["failed"], 1);
    assert_eq!(counts["warnings"], 1);
    assert_eq!(counts["errors"], 1);
    assert_eq!(counts["skipped"], 1);
    let partition: u64 = ["passed", "failed", "warnings", "errors"]
        .iter()
        .map(|key| counts[*key].as_u64().expect("a count"))
        .sum();
    assert_eq!(
        Some(partition),
        counts["total"].as_u64(),
        "the outcome counts sum to the total"
    );

    // The round trip keeps every identity, and aggregation sees exactly the
    // distinct identities: five results plus one skip.
    let parsed = render_then_parse(&input).expect("every identity is unique");
    assert_eq!(parsed.results.len(), 5);
    assert_eq!(parsed.skipped.len(), 1);
    let comparison =
        compare(&parsed, &parsed, CompareOptions::default()).expect("a unique report aggregates");
    assert_eq!(
        comparison.entries.len(),
        6,
        "one entry per distinct identity"
    );
    assert!(comparison
        .entries
        .iter()
        .all(|entry| matches!(entry.classification, Classification::Unchanged)));
    assert_eq!(comparison.summary.unchanged, 6);
    assert_eq!(comparison.summary.regressed, 0);
    assert_eq!(comparison.verdict, Verdict::NoChange);
    let shared: Vec<&str> = comparison
        .entries
        .iter()
        .filter(|entry| entry.id == "p28-shared-1")
        .map(|entry| entry.surface.as_str())
        .collect();
    assert_eq!(
        shared,
        vec!["rpc", "xdr"],
        "the same id on two surfaces stays two entries, sorted by surface"
    );
}
