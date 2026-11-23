//! Backwards compatibility tests for historical JSON report formats.
//!
//! Verifies that execution reports emitted by previous engine versions (v0.1.0, v0.1.1,
//! v0.2.0 and v0.3.0) continue to parse cleanly into the current [`ReportInput`], ensuring
//! that viewer and report comparison tools remain backwards compatible with historical
//! records.
//!
//! # Provenance of the v0.2.0 and v0.3.0 golden reports
//!
//! `v0.2.0-real.json` and `v0.3.0-real.json` were captured on 2026-11-23 from the published
//! Linux x86_64 release binaries and are stored byte-for-byte as the binaries emitted them.
//! Nothing was hand-written or post-edited. To reproduce a capture:
//!
//! 1. Download the release asset and `SHA256SUMS` for the version from
//!    `https://github.com/StellarCanary/Protocol-Canary/releases/download/<tag>/`.
//! 2. Verify the binary against the release checksum manifest (`sha256sum -c`), then
//!    record `stellar-canary-linux-x86_64 version` output.
//! 3. Check out `StellarCanary/ProtocolCanary-Fixtures` at the immutable `protocol-28` tag,
//!    commit `75ec2c293a44f09d9c8c1f76722a24a2334ac20a`.
//! 4. Run the binary from a directory whose basename matches the report's `project.name`
//!    (`v0.2.0` or `v0.3.0`), because that name is derived from the working directory.
//!
//! | Item | v0.2.0 | v0.3.0 |
//! |---|---|---|
//! | Asset URL | `.../v0.2.0/stellar-canary-linux-x86_64` | `.../v0.3.0/stellar-canary-linux-x86_64` |
//! | Binary SHA-256 | `34656189e15169a9f9d339288f7a8a95bb81d86ecee14ca278a165d041d28943` | `b19d5ce5a9da67ab0690720c558ca658c4eecadda61250322033dbda8862ef43` |
//! | `version` output | `stellar-canary 0.2.0` | `stellar-canary 0.3.0` |
//! | Report SHA-256 | `9bed7efb5057721873d10842b45f85f7e06822d9504ca101b115b61887aab4d4` | `5cacc0207313d57242fda0045e66f3bf92f01ae27b4f23b79ed7b031f8586fac` |
//! | Exit status | 0 | 0 |
//!
//! v0.2.0 predates `--surface`, so it ran with a temporary offline config named
//! `offline-xdr.toml` (not checked in) containing:
//!
//! ```toml
//! version = 1
//! protocol = 28
//!
//! [tests]
//! xdr = true
//! rpc = false
//! soroban = false
//! ```
//!
//! The exact commands were:
//!
//! ```text
//! # v0.2.0
//! ./stellar-canary-linux-x86_64 check --config offline-xdr.toml \
//!   --fixtures-dir /absolute/path/ProtocolCanary-Fixtures/protocol-28 \
//!   --protocol 28 --json --no-cache > v0.2.0-real.json
//!
//! # v0.3.0
//! ./stellar-canary-linux-x86_64 check \
//!   --fixtures-dir /absolute/path/ProtocolCanary-Fixtures/protocol-28 \
//!   --protocol 28 --surface xdr --json --no-cache > v0.3.0-real.json
//! ```

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

// Captured from the published v0.2.0 and v0.3.0 release binaries; see the module
// documentation for the exact assets, hashes, commands and fixture-pack commit.
const V020_REAL: &str = include_str!("fixtures/golden/v0.2.0-real.json");
const V030_REAL: &str = include_str!("fixtures/golden/v0.3.0-real.json");

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

/// Asserts that every top-level `counts` value in the raw JSON survived into the
/// parsed report: the parsed results and skipped entries are the same length as
/// the raw arrays, and each count matches the corresponding status tally.
fn assert_counts_match(raw_json: &str, expected: (u64, u64, u64, u64, u64, u64)) -> JsonCountsView {
    let raw: serde_json::Value = serde_json::from_str(raw_json).expect("valid JSON");
    let counts = &raw["counts"];
    let view = JsonCountsView {
        total: counts["total"].as_u64().expect("counts.total"),
        passed: counts["passed"].as_u64().expect("counts.passed"),
        failed: counts["failed"].as_u64().expect("counts.failed"),
        warnings: counts["warnings"].as_u64().expect("counts.warnings"),
        errors: counts["errors"].as_u64().expect("counts.errors"),
        skipped: counts["skipped"].as_u64().expect("counts.skipped"),
    };
    assert_eq!(
        (
            view.total,
            view.passed,
            view.failed,
            view.warnings,
            view.errors,
            view.skipped
        ),
        expected,
        "top-level counts must match the captured release output"
    );
    view
}

/// The raw count fields, read back from the fixture JSON so the test compares
/// the captured document against itself and against the parsed report.
#[derive(Debug)]
struct JsonCountsView {
    total: u64,
    passed: u64,
    failed: u64,
    warnings: u64,
    errors: u64,
    skipped: u64,
}

/// Asserts that every field of every raw `results` entry was preserved by the
/// parser, not just the fields this test happens to care about.
fn assert_results_match_raw_json(raw_json: &str, report: &canary_report::ReportInput) {
    let raw: serde_json::Value = serde_json::from_str(raw_json).expect("valid JSON");
    let raw_results = raw["results"].as_array().expect("results array");
    assert_eq!(
        raw_results.len(),
        report.results.len(),
        "no fixture result entry may be dropped"
    );

    for (raw_result, parsed) in raw_results.iter().zip(&report.results) {
        assert_eq!(raw_result["testId"], parsed.test_id);
        assert_eq!(
            raw_result["protocol"].as_u64().expect("protocol"),
            u64::from(parsed.protocol.0)
        );
        assert_eq!(raw_result["surface"], parsed.surface.to_string());
        assert_eq!(raw_result["status"], parsed.status.to_string());
        assert_eq!(raw_result["summary"], parsed.summary);
        assert_eq!(
            raw_result["durationMs"].as_u64().expect("durationMs"),
            parsed.duration_ms
        );
        assert_eq!(
            raw_result["fixtureId"].as_str(),
            parsed.fixture_id.as_deref()
        );
        assert_eq!(
            raw_result["source"].as_str().expect("source"),
            parsed.source.as_str()
        );
        match raw_result.get("details") {
            None | Some(serde_json::Value::Null) => assert!(parsed.details.is_none()),
            Some(serde_json::Value::String(details)) => {
                assert_eq!(Some(details.as_str()), parsed.details.as_deref());
            }
            other => panic!("unexpected details value: {other:?}"),
        }
    }
}

/// Asserts that the skipped array survives parsing with its id, surface and
/// reason intact.
fn assert_skipped_match_raw_json(raw_json: &str, report: &canary_report::ReportInput) {
    let raw: serde_json::Value = serde_json::from_str(raw_json).expect("valid JSON");
    // The v0.3.0 writer omits `skipped` entirely when the array is empty, so
    // an absent field and an explicit empty array are both "nothing skipped".
    let raw_skipped: &[serde_json::Value] = match raw.get("skipped") {
        None | Some(serde_json::Value::Null) => &[],
        Some(serde_json::Value::Array(items)) => items,
        Some(other) => panic!("unexpected skipped value: {other:?}"),
    };
    assert_eq!(
        raw_skipped.len(),
        report.skipped.len(),
        "no skipped entry may be dropped"
    );

    for (raw_skip, parsed) in raw_skipped.iter().zip(&report.skipped) {
        assert_eq!(raw_skip["fixtureId"], parsed.fixture_id);
        assert_eq!(raw_skip["surface"], parsed.surface.to_string());
        assert_eq!(raw_skip["reason"], parsed.reason);
    }
}

/// v0.2.0 introduced `counts` and recorded result `source`, but predates the
/// `run` provenance block. The captured report must parse fully, including its
/// two skipped surfaces from the offline configuration.
#[test]
fn test_v020_real_report_parses() {
    let report = JsonReporter::parse(V020_REAL).expect("v0.2.0 real report should parse");
    assert_eq!(report.tool_version, "0.2.0");
    assert_eq!(report.target_protocol, ProtocolVersion(28));
    assert_eq!(report.project.name, "v0.2.0");
    assert_eq!(report.overall_status(), ReportStatus::Pass);
    assert_eq!(report.results.len(), 3);
    assert_eq!(report.skipped.len(), 2);
    assert!(report.network.is_none());
    assert_eq!(report.git.commit, None);
    assert_eq!(report.git.is_dirty, Some(false));
    assert!(
        report.run.is_none(),
        "v0.2.0 predates the run provenance block"
    );

    let counts = assert_counts_match(V020_REAL, (3, 3, 0, 0, 0, 2));
    assert_eq!(counts.total as usize, report.results.len());
    assert_eq!(counts.skipped as usize, report.skipped.len());
    assert_results_match_raw_json(V020_REAL, &report);
    assert_skipped_match_raw_json(V020_REAL, &report);

    // Every executed result was live (not replayed from the cache), and the
    // only non-XDR surfaces present are the two disabled ones in `skipped`.
    assert!(report
        .results
        .iter()
        .all(|result| result.source == ResultSource::Live));
    let skipped_ids: Vec<&str> = report
        .skipped
        .iter()
        .map(|skip| skip.fixture_id.as_str())
        .collect();
    assert_eq!(
        skipped_ids,
        ["p28-rpc-network", "p28-soroban-native-asset-name"]
    );
}

/// v0.3.0 adds the `run` provenance block with the build commit of the
/// release binary. The captured report must preserve it alongside the same
/// result and count structure v0.2.0 introduced.
#[test]
fn test_v030_real_report_parses() {
    let report = JsonReporter::parse(V030_REAL).expect("v0.3.0 real report should parse");
    assert_eq!(report.tool_version, "0.3.0");
    assert_eq!(report.target_protocol, ProtocolVersion(28));
    assert_eq!(report.project.name, "v0.3.0");
    assert_eq!(report.overall_status(), ReportStatus::Pass);
    assert_eq!(report.results.len(), 3);
    assert!(report.skipped.is_empty(), "--surface xdr skips no fixtures");
    assert!(report.network.is_none());
    assert_eq!(report.git.commit, None);
    assert_eq!(report.git.is_dirty, Some(false));

    let run = report
        .run
        .as_ref()
        .expect("v0.3.0 must record run provenance");
    assert_eq!(
        run.tool_build_commit.as_deref(),
        Some("464c45d1bb100a009bb9dab9847c2c89ad033ab2")
    );

    let counts = assert_counts_match(V030_REAL, (3, 3, 0, 0, 0, 0));
    assert_eq!(counts.total as usize, report.results.len());
    assert_eq!(counts.skipped as usize, report.skipped.len());
    assert_results_match_raw_json(V030_REAL, &report);
    assert_skipped_match_raw_json(V030_REAL, &report);

    assert!(report
        .results
        .iter()
        .all(|result| result.source == ResultSource::Live));
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
        V020_REAL,
        V030_REAL,
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
            ["0.1.0", "0.1.1", "0.2.0", "0.3.0"].contains(&parsed.tool_version.as_str()),
            "Unexpected toolVersion {} in {:?}",
            parsed.tool_version,
            entry.path()
        );
    }
}
