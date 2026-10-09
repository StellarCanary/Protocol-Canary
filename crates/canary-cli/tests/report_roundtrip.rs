mod support;

use support::{run_in, stderr, stdout, TempProject};

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
}
