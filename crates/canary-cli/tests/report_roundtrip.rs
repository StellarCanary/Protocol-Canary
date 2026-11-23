mod support;

use support::{run_in, stderr, stdout, TempProject};

use canary_core::{
    CompatibilityResult, GitContext, NetworkName, PolicyDecision, ProjectType, ProtocolVersion,
    ResultSource, SkipCode, Status, Surface,
};
use canary_report::{
    JsonReporter, NetworkSummary, ProjectSummary, ReportInput, RunSummary, SkipSummary,
};
use serde_json::Value;

const FIXTURE: &str = "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"\n";

const OFFLINE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = true
rpc = false
soroban = false
"#;

#[test]
fn report_renders_a_saved_json_report_as_markdown_without_touching_the_network() {
    let dir = TempProject::new("report-roundtrip");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", FIXTURE);

    let check_output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(check_output.status.code(), Some(0));
    dir.write("result.json", &stdout(&check_output));

    let report_output = run_in(
        &dir.path,
        &["report", "result.json", "--format", "markdown"],
    );
    assert_eq!(report_output.status.code(), Some(0));
    let text = stdout(&report_output);
    assert!(text.contains("## Stellar Protocol Canary"));
    assert!(text.contains("**Result: PASS**"));
}

/// `report --format junit` re-renders a stored JSON report as JUnit XML
/// for CI test-summary dashboards, without re-running anything.
#[test]
fn report_renders_a_saved_json_report_as_junit_xml() {
    let dir = TempProject::new("report-junit");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", FIXTURE);

    let check_output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(check_output.status.code(), Some(0));
    dir.write("result.json", &stdout(&check_output));

    let report_output = run_in(&dir.path, &["report", "result.json", "--format", "junit"]);
    assert_eq!(report_output.status.code(), Some(0));
    let text = stdout(&report_output);
    assert!(
        text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        "junit output must start with the XML declaration, got: {text}"
    );
    assert!(text.contains("<testsuites name=\"stellar-canary\""));
    assert!(text.contains("name=\"p28-xdr-1\""));
    assert!(text.trim_end().ends_with("</testsuites>"));
}

/// `ReportArgs::format` documents Markdown as the default, so a bare
/// `report <path>` invocation must render Markdown rather than terminal
/// output. This covers the default that the explicit-`--format markdown`
/// test above does not.
#[test]
fn report_defaults_to_markdown_when_no_format_flag_is_given() {
    let dir = TempProject::new("report-default-format");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", FIXTURE);

    let check_output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(check_output.status.code(), Some(0));
    dir.write("result.json", &stdout(&check_output));

    let report_output = run_in(&dir.path, &["report", "result.json"]);
    assert_eq!(report_output.status.code(), Some(0));
    let text = stdout(&report_output);
    assert!(
        text.starts_with("## Stellar Protocol Canary"),
        "report must default to Markdown output, got: {text}"
    );
    // The Markdown table delimiter is another Markdown-only marker: the
    // Terminal and JSON reporters never emit a `|---|` ruler, so this
    // asserts the shape of the output, not just a single coincidental
    // substring.
    assert!(
        text.contains("|---|---|"),
        "default output must be Markdown-shaped, got: {text}"
    );
    assert!(text.contains("**Result: PASS**"));
}

/// `--format terminal` is the one output format that no binary-level test
/// rendered: the unit test in `commands.rs` iterates every format but only
/// asserts the exit code, so a regression that emitted Markdown or JSON for
/// the terminal branch would pass it. The terminal layout is also the only
/// one carrying the per-surface summary and the `Status:` verdict line.
#[test]
fn report_renders_a_saved_json_report_as_terminal_output_when_asked() {
    let dir = TempProject::new("report-terminal-format");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", FIXTURE);

    let check_output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(check_output.status.code(), Some(0));
    dir.write("result.json", &stdout(&check_output));

    let report_output = run_in(
        &dir.path,
        &["report", "result.json", "--format", "terminal"],
    );
    assert_eq!(report_output.status.code(), Some(0));
    let text = stdout(&report_output);

    assert!(
        text.starts_with("Stellar Protocol Canary\n"),
        "terminal output must start with the plain terminal banner, got: {text}"
    );
    // The two other formats' signatures must be absent.
    assert!(
        !text.contains("## Stellar Protocol Canary"),
        "--format terminal must not render Markdown; stdout: {text}"
    );
    assert!(
        !text.contains("**Result: PASS**"),
        "--format terminal must not render Markdown; stdout: {text}"
    );
    assert!(
        !text.contains("\"test_id\""),
        "--format terminal must not render JSON; stdout: {text}"
    );

    // Terminal-only content: the summary line and the policy verdict.
    assert!(text.contains("applicable checks passed."), "stdout: {text}");
    assert!(text.contains("Status: PASS"), "stdout: {text}");
}

#[test]
fn report_on_a_malformed_file_exits_with_a_configuration_error() {
    let dir = TempProject::new("report-malformed");
    dir.write("result.json", "not json at all");

    let output = run_in(&dir.path, &["report", "result.json"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(
        err.contains("error: configuration error: invalid report file:"),
        "stderr: {err}"
    );
    // A malformed file gets no hint: the message alone says what is wrong.
    assert!(!err.contains("Hint:"), "stderr: {err}");
}

#[test]
fn a_report_from_another_schema_version_is_reported_with_an_actionable_hint() {
    let dir = TempProject::new("report-schema-mismatch");
    dir.write(
        "result.json",
        "{\n  \"schemaVersion\": 999,\n  \"toolVersion\": \"9.9.9\",\n  \"targetProtocol\": 28,\n  \"project\": { \"name\": \"x\", \"type\": \"unknown\" },\n  \"status\": \"pass\",\n  \"results\": []\n}\n",
    );

    let output = run_in(&dir.path, &["report", "result.json"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(err.contains("unsupported schema version"), "stderr: {err}");
    assert!(err.contains("Hint:"), "stderr: {err}");
    assert!(err.contains("engine version"), "stderr: {err}");
}

/// A run carrying the fields the JSON report is most likely to truncate or
/// normalize away: multi-line failure diagnostics, structured skip evidence,
/// a non-trivial verdict mix and a duration beyond IEEE-754's exact integer
/// range.
fn detailed_report() -> ReportInput {
    let multi_line_failure = "failure evidence:\n  expected: 4 XDR bytes\n  actual:   3 XDR bytes\n\tat 2026-11-23T08:57:31Z\n  note: \"backslash\\ and unicode ✓ survived\"".to_string();

    ReportInput {
        tool_version: "0.3.0".into(),
        target_protocol: ProtocolVersion(28),
        project: ProjectSummary {
            name: "roundtrip-project".into(),
            project_type: ProjectType::Soroban,
        },
        network: Some(NetworkSummary {
            name: NetworkName::Testnet,
            observed_protocol: Some(ProtocolVersion(28)),
            error: Some("gateway\nreturned\tHTTP 504".into()),
        }),
        results: vec![
            CompatibilityResult {
                test_id: "p28-xdr-pass".into(),
                protocol: ProtocolVersion(28),
                surface: Surface::Xdr,
                status: Status::Pass,
                summary: "decoded successfully".into(),
                details: Some("first line\nsecond line\n\tindented third line".into()),
                duration_ms: 0,
                fixture_id: Some("p28-xdr-pass".into()),
                source: ResultSource::Live,
            },
            CompatibilityResult {
                test_id: "p28-rpc-fail".into(),
                protocol: ProtocolVersion(28),
                surface: Surface::Rpc,
                status: Status::Fail,
                summary: "response did not match the contract".into(),
                details: Some(multi_line_failure.clone()),
                duration_ms: 12,
                fixture_id: Some("p28-rpc-fail".into()),
                source: ResultSource::Cache,
            },
            CompatibilityResult {
                test_id: "p28-soroban-error".into(),
                protocol: ProtocolVersion(28),
                surface: Surface::Soroban,
                status: Status::Error,
                summary: "execution error".into(),
                details: Some(
                    "panic: assertion failed\n  at fixture line 7\n\nbacktrace omitted".into(),
                ),
                // Larger than 2^53: a float roundtrip would silently change it.
                duration_ms: 9_007_199_254_740_993,
                fixture_id: Some("p28-soroban-error".into()),
                source: ResultSource::Live,
            },
        ],
        skipped: vec![
            SkipSummary {
                fixture_id: "p28-soroban-skip".into(),
                surface: Surface::Soroban,
                reason: "requires a capability not declared by this project:\n  contractimport"
                    .into(),
                code: SkipCode::MissingCapability,
            },
            SkipSummary {
                fixture_id: "p27-xdr-old".into(),
                surface: Surface::Xdr,
                reason: "fixture targets protocol 27, this run targets protocol 28".into(),
                code: SkipCode::UnsupportedProtocol,
            },
        ],
        verification: Vec::new(),
        decision: PolicyDecision::Fail,
        git: GitContext {
            commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
            branch: Some("test/741-report-roundtrip-serialization".into()),
            is_dirty: Some(true),
        },
        verbose: true,
        run: Some(RunSummary {
            tool_build_commit: Some("fedcba9876543210fedcba9876543210fedcba98".into()),
        }),
    }
}

/// Every value in a report survives `render -> parse`: multi-line details,
/// skip reasons/codes, the verdict-bearing status, git/run provenance and
/// the exact source of each result. This is the `ReportInput` equivalent of
/// asserting `ExecutionRecord`/`Verdict` field parity: the report wire format
/// represents a result's verdict as its `status`, and the execution record as
/// `ReportInput` itself (there are no separate types by those names).
#[test]
fn report_json_roundtrip_preserves_multiline_diagnostics_and_failure_evidence() {
    let original = detailed_report();
    let first = JsonReporter::render(&original);

    let parsed = JsonReporter::parse(&first).expect("rendered report must parse");

    assert_eq!(parsed.tool_version, original.tool_version);
    assert_eq!(parsed.target_protocol, original.target_protocol);
    assert_eq!(parsed.project, original.project);
    assert_eq!(parsed.network, original.network);
    assert_eq!(parsed.results, original.results);
    assert_eq!(parsed.skipped, original.skipped);
    assert_eq!(parsed.git, original.git);
    assert_eq!(parsed.verbose, original.verbose);
    assert_eq!(parsed.run, original.run);
    assert_eq!(parsed.overall_status(), original.overall_status());
    assert_eq!(parsed.decision, PolicyDecision::Fail);

    // The diagnostics are not flattened, escaped twice, or truncated.
    let failure = &parsed.results[1];
    assert_eq!(failure.status, Status::Fail);
    assert_eq!(
        failure.details.as_deref(),
        original.results[1].details.as_deref()
    );
    assert_eq!(
        failure
            .details
            .as_deref()
            .map(str::lines)
            .map(|l| l.count()),
        Some(5)
    );
    assert!(failure
        .details
        .as_deref()
        .unwrap()
        .contains("\tat 2026-11-23T08:57:31Z"));

    // Structured skip evidence keeps both its free-form reason and its
    // stable machine-readable code.
    assert_eq!(parsed.skipped[0].code, SkipCode::MissingCapability);
    assert_eq!(parsed.skipped[1].code, SkipCode::UnsupportedProtocol);
    assert!(parsed.skipped[0].reason.contains('\n'));

    // Durations roundtrip as exact integers, including values a floating
    // point representation cannot hold.
    assert_eq!(parsed.results[2].duration_ms, 9_007_199_254_740_993);
    assert_eq!(parsed.results[1].duration_ms, 12);

    // Timestamp-shaped text is opaque diagnostic content and must be byte
    // for byte identical after the cycle.
    assert!(parsed.results[1]
        .details
        .as_deref()
        .unwrap()
        .contains("2026-11-23T08:57:31Z"));
}

/// A second serialization cycle must be byte-identical to the first: no
/// field is dropped or rewritten, so rendering a parsed report is stable.
#[test]
fn report_json_roundtrip_is_stable_across_two_serialization_cycles() {
    let original = detailed_report();
    let first = JsonReporter::render(&original);

    let parsed = JsonReporter::parse(&first).expect("first parse");
    let second = JsonReporter::render(&parsed);
    assert_eq!(first, second, "the first and second renders diverged");

    let reparsed = JsonReporter::parse(&second).expect("second parse");
    assert_eq!(reparsed.results, original.results);
    assert_eq!(reparsed.skipped, original.skipped);

    // The raw wire document keeps the multi-line details verbatim and the
    // large duration as a JSON integer, not a rounded float.
    let doc: Value = serde_json::from_str(&first).expect("rendered JSON is valid");
    assert_eq!(doc["results"][1]["status"], "fail");
    assert_eq!(
        doc["results"][1]["details"],
        original.results[1].details.clone().unwrap()
    );
    assert!(doc["results"][1]["details"]
        .as_str()
        .unwrap()
        .contains('\n'));
    assert_eq!(doc["results"][2]["durationMs"], 9_007_199_254_740_993_u64);
    assert_eq!(doc["counts"]["total"], 3);
    assert_eq!(doc["counts"]["failed"], 1);
    assert_eq!(doc["counts"]["errors"], 1);
    assert_eq!(doc["counts"]["skipped"], 2);
    assert_eq!(doc["status"], "error");
}
