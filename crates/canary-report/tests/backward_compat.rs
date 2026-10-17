//! Backwards compatibility tests for historical JSON report formats.
//!
//! Verifies that execution reports emitted by previous engine versions (v0.1.0 and v0.1.1)
//! continue to parse cleanly into the current [`ReportInput`], ensuring that viewer and report
//! comparison tools remain backwards compatible with historical records.

use canary_core::{ProtocolVersion, ResultSource, Status, Surface};
use canary_report::json::SCHEMA_VERSION;
use canary_report::{JsonReporter, ReportStatus};
use std::path::Path;

// Embedded golden reports generated from real v0.1.0 and v0.1.1 engine binaries
const V010_PASS: &str = include_str!("fixtures/golden/v0.1.0-pass.json");
const V010_FAIL: &str = include_str!("fixtures/golden/v0.1.0-fail.json");
const V010_SKIPPED: &str = include_str!("fixtures/golden/v0.1.0-skipped.json");
const V010_MULTI: &str = include_str!("fixtures/golden/v0.1.0-multi.json");
const V010_NO_GIT: &str = include_str!("fixtures/golden/v0.1.0-no-git.json");

const V011_PASS: &str = include_str!("fixtures/golden/v0.1.1-pass.json");
const V011_FAIL: &str = include_str!("fixtures/golden/v0.1.1-fail.json");
const V011_SKIPPED: &str = include_str!("fixtures/golden/v0.1.1-skipped.json");
const V011_MULTI: &str = include_str!("fixtures/golden/v0.1.1-multi.json");
const V011_NO_GIT: &str = include_str!("fixtures/golden/v0.1.1-no-git.json");

#[test]
fn test_v010_pass_report_parses() {
    let report = JsonReporter::parse(V010_PASS).expect("v0.1.0 pass report should parse");
    assert_eq!(report.tool_version, "0.1.0");
    assert_eq!(report.target_protocol, ProtocolVersion(28));
    assert_eq!(report.overall_status(), ReportStatus::Pass);
    assert_eq!(report.results.len(), 1);

    let res = &report.results[0];
    assert_eq!(res.test_id, "p28-xdr-valid");
    assert_eq!(res.protocol, ProtocolVersion(28));
    assert_eq!(res.surface, Surface::Xdr);
    assert_eq!(res.status, Status::Pass);
    assert!(res.summary.contains("StellarValue"));

    // Verify git fields are populated
    assert!(report.git.commit.is_some());
    assert_eq!(report.git.is_dirty, Some(false));
}

#[test]
fn test_v010_fail_report_parses() {
    let report = JsonReporter::parse(V010_FAIL).expect("v0.1.0 fail report should parse");
    assert_eq!(report.tool_version, "0.1.0");
    assert_eq!(report.target_protocol, ProtocolVersion(28));
    assert_eq!(report.overall_status(), ReportStatus::Fail);
    assert_eq!(report.results.len(), 1);

    let res = &report.results[0];
    assert_eq!(res.test_id, "p28-xdr-invalid");
    assert_eq!(res.status, Status::Fail);
    assert!(res.details.is_some());
}

#[test]
fn test_v010_skipped_report_parses() {
    let report = JsonReporter::parse(V010_SKIPPED).expect("v0.1.0 skipped report should parse");
    assert_eq!(report.tool_version, "0.1.0");
    assert_eq!(report.overall_status(), ReportStatus::Pass);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.skipped.len(), 1);

    let skip = &report.skipped[0];
    assert_eq!(skip.fixture_id, "p27-xdr-skipped");
    assert_eq!(skip.surface, Surface::Xdr);
    assert!(skip.reason.contains("protocol"));
}

#[test]
fn test_v010_multi_result_report_parses() {
    let report = JsonReporter::parse(V010_MULTI).expect("v0.1.0 multi report should parse");
    assert_eq!(report.tool_version, "0.1.0");
    assert_eq!(report.overall_status(), ReportStatus::Fail);
    assert_eq!(report.results.len(), 2);

    let statuses: Vec<Status> = report.results.iter().map(|r| r.status).collect();
    assert!(statuses.contains(&Status::Pass));
    assert!(statuses.contains(&Status::Fail));
}

#[test]
fn test_v010_no_git_report_parses() {
    let report = JsonReporter::parse(V010_NO_GIT).expect("v0.1.0 no-git report should parse");
    assert_eq!(report.tool_version, "0.1.0");
    assert_eq!(report.overall_status(), ReportStatus::Pass);
    assert_eq!(report.git.commit, None);
    assert_eq!(report.git.branch, None);
    assert_eq!(report.git.is_dirty, Some(false));
}

#[test]
fn test_v011_pass_report_parses() {
    let report = JsonReporter::parse(V011_PASS).expect("v0.1.1 pass report should parse");
    assert_eq!(report.tool_version, "0.1.1");
    assert_eq!(report.target_protocol, ProtocolVersion(28));
    assert_eq!(report.overall_status(), ReportStatus::Pass);
    assert_eq!(report.results.len(), 1);

    let res = &report.results[0];
    assert_eq!(res.test_id, "p28-xdr-valid");
    assert_eq!(res.status, Status::Pass);
}

#[test]
fn test_v011_fail_report_parses() {
    let report = JsonReporter::parse(V011_FAIL).expect("v0.1.1 fail report should parse");
    assert_eq!(report.tool_version, "0.1.1");
    assert_eq!(report.overall_status(), ReportStatus::Fail);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].status, Status::Fail);
}

#[test]
fn test_v011_skipped_report_parses() {
    let report = JsonReporter::parse(V011_SKIPPED).expect("v0.1.1 skipped report should parse");
    assert_eq!(report.tool_version, "0.1.1");
    assert_eq!(report.overall_status(), ReportStatus::Pass);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].fixture_id, "p27-xdr-skipped");
}

#[test]
fn test_v011_multi_result_report_parses() {
    let report = JsonReporter::parse(V011_MULTI).expect("v0.1.1 multi report should parse");
    assert_eq!(report.tool_version, "0.1.1");
    assert_eq!(report.overall_status(), ReportStatus::Fail);
    assert_eq!(report.results.len(), 2);
}

#[test]
fn test_v011_no_git_report_parses() {
    let report = JsonReporter::parse(V011_NO_GIT).expect("v0.1.1 no-git report should parse");
    assert_eq!(report.tool_version, "0.1.1");
    assert_eq!(report.overall_status(), ReportStatus::Pass);
    assert_eq!(report.git.commit, None);
    assert_eq!(report.git.branch, None);
}

/// Negative testing: asserts that optional fields not present in older report versions
/// parse cleanly as `None` (or neutral defaults) without raising deserialization errors.
#[test]
fn negative_missing_optional_fields_parse_as_none_or_defaults() {
    let all_golden = [
        ("v0.1.0-pass", V010_PASS),
        ("v0.1.0-fail", V010_FAIL),
        ("v0.1.0-skipped", V010_SKIPPED),
        ("v0.1.0-multi", V010_MULTI),
        ("v0.1.0-no-git", V010_NO_GIT),
        ("v0.1.1-pass", V011_PASS),
        ("v0.1.1-fail", V011_FAIL),
        ("v0.1.1-skipped", V011_SKIPPED),
        ("v0.1.1-multi", V011_MULTI),
        ("v0.1.1-no-git", V011_NO_GIT),
    ];

    for (name, json_str) in all_golden {
        let parsed =
            JsonReporter::parse(json_str).unwrap_or_else(|e| panic!("Failed to parse {name}: {e}"));

        // 1. `run` provenance (E-055, CF-01) was introduced after v0.1.1; must parse as None
        assert!(
            parsed.run.is_none(),
            "{name}: expected run provenance to parse as None in older report"
        );

        // 2. `network` was absent in offline runs; must parse as None
        assert!(
            parsed.network.is_none(),
            "{name}: expected network to parse as None for offline run"
        );

        // 3. `source` (`live` | `cache`) was introduced after v0.1.1 (RH-02);
        // absent in older reports, which must parse as ResultSource::Unknown
        for result in &parsed.results {
            assert_eq!(
                result.source,
                ResultSource::Unknown,
                "{name}: result {} source must parse as ResultSource::Unknown",
                result.test_id
            );
        }

        // 4. `verbose` was introduced in later CLI versions; must default to false
        assert!(
            !parsed.verbose,
            "{name}: verbose should default to false for older reports"
        );
    }
}

/// Negative testing: asserts specifically that missing `counts` in v0.1.0 does not break parsing.
#[test]
fn negative_v010_missing_counts_field_accepted() {
    // Verify that the v0.1.0 JSON text genuinely does not contain "counts"
    assert!(
        !V010_PASS.contains("\"counts\""),
        "Authentic v0.1.0 report should predate counts field"
    );

    // Assert that parser accepts the document without failing
    let parsed = JsonReporter::parse(V010_PASS).expect("v0.1.0 without counts must parse");
    assert_eq!(parsed.results.len(), 1);
    assert_eq!(parsed.overall_status(), ReportStatus::Pass);
}

/// Verifies that historical reports can be re-rendered to valid JSON and roundtripped.
#[test]
fn test_historical_reports_rerender_and_reparse() {
    let all_golden = [
        V010_PASS,
        V010_FAIL,
        V010_SKIPPED,
        V010_MULTI,
        V010_NO_GIT,
        V011_PASS,
        V011_FAIL,
        V011_SKIPPED,
        V011_MULTI,
        V011_NO_GIT,
    ];

    for original_json in all_golden {
        let first_pass = JsonReporter::parse(original_json).expect("first parse succeeds");
        let rendered = JsonReporter::render(&first_pass);
        let second_pass = JsonReporter::parse(&rendered).expect("re-rendered parse succeeds");

        assert_eq!(first_pass.tool_version, second_pass.tool_version);
        assert_eq!(first_pass.target_protocol, second_pass.target_protocol);
        assert_eq!(first_pass.overall_status(), second_pass.overall_status());
        assert_eq!(first_pass.results.len(), second_pass.results.len());
        assert_eq!(first_pass.skipped.len(), second_pass.skipped.len());
    }
}

/// Verifies that all files in `crates/canary-report/tests/fixtures/golden` exist and parse cleanly.
#[test]
fn test_all_golden_corpus_files_exist_and_parse() {
    let golden_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden");

    assert!(
        golden_dir.exists(),
        "golden directory must exist at {:?}",
        golden_dir
    );

    let entries: Vec<_> = std::fs::read_dir(&golden_dir)
        .expect("read golden directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();

    assert!(
        entries.len() >= 10,
        "expected at least 10 golden report files, found {}",
        entries.len()
    );

    for entry in entries {
        let content = std::fs::read_to_string(entry.path())
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {e}", entry.path()));

        let val: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Invalid JSON in {:?}: {e}", entry.path()));

        assert_eq!(
            val["schemaVersion"],
            SCHEMA_VERSION,
            "{:?} must have schemaVersion 1",
            entry.path()
        );

        let parsed = JsonReporter::parse(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {e}", entry.path()));

        assert!(
            parsed.tool_version.starts_with("0.1.0") || parsed.tool_version.starts_with("0.1.1"),
            "Unexpected toolVersion {} in {:?}",
            parsed.tool_version,
            entry.path()
        );
    }
}
