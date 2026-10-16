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
        }],
    );

    let parsed = render_then_parse(&input).expect("skip list is not part of the identity rule");
    assert_eq!(parsed.results.len(), 1);
    assert_eq!(parsed.skipped.len(), 1);
}
