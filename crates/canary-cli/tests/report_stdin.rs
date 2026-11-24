//! `stellar-canary report -` reads report JSON from standard input (E-053).
//!
//! These tests pipe a real report through the binary's stdin the way
//! `stellar-canary check --json | stellar-canary report -` would, and cover
//! the negative side too: the 10 MiB input cap approved under D-06 must
//! abort the read instead of buffering or truncating an oversized stream.

mod support;

use support::{run_in, run_in_with_stdin, stderr, stdout, TempProject};

const FIXTURE: &str = "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"\n";

const OFFLINE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = true
rpc = false
soroban = false
"#;

/// Runs `check --json` in a fresh offline project and returns the JSON
/// report on stdout.
fn generated_report(dir: &TempProject) -> String {
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", FIXTURE);

    let check_output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(check_output.status.code(), Some(0));
    stdout(&check_output)
}

#[test]
fn report_reads_a_json_report_from_stdin_and_renders_markdown_by_default() {
    let dir = TempProject::new("report-stdin-markdown");
    let json = generated_report(&dir);

    let output = run_in_with_stdin(&dir.path, &["report", "-"], json.as_bytes());
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));

    let text = stdout(&output);
    assert!(
        text.starts_with("## Stellar Protocol Canary"),
        "stdin input must render Markdown by default, got: {text}"
    );
    assert!(text.contains("|---|---|"), "got: {text}");
    assert!(text.contains("**Result: PASS**"), "got: {text}");
}

#[test]
fn report_reads_stdin_and_formats_it_as_terminal_output() {
    let dir = TempProject::new("report-stdin-terminal");
    let json = generated_report(&dir);

    let output = run_in_with_stdin(
        &dir.path,
        &["report", "-", "--format", "terminal"],
        json.as_bytes(),
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));

    let text = stdout(&output);
    assert!(
        text.starts_with("Stellar Protocol Canary\n"),
        "terminal output must start with the plain banner, got: {text}"
    );
    assert!(text.contains("applicable checks passed."), "got: {text}");
    assert!(text.contains("Status: PASS"), "got: {text}");

    // The other two formats' signatures must be absent.
    assert!(!text.contains("## Stellar Protocol Canary"), "got: {text}");
    assert!(!text.contains("\"test_id\""), "got: {text}");
}

/// The pipeline the issue exists for: a report produced on stdout of one
/// command is re-rendered by the next without touching a file.
#[test]
fn report_chains_after_check_json_on_a_pipe() {
    let dir = TempProject::new("report-stdin-pipe");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", FIXTURE);

    let check_output = run_in(&dir.path, &["check", "--format", "json"]);
    assert_eq!(check_output.status.code(), Some(0));
    let json = stdout(&check_output);

    let report_output = run_in_with_stdin(
        &dir.path,
        &["report", "-", "--format", "json"],
        json.as_bytes(),
    );
    assert_eq!(
        report_output.status.code(),
        Some(0),
        "stderr: {}",
        stderr(&report_output)
    );
    let text = stdout(&report_output);
    // JSON re-rendered from stdin must be parseable, round-shaped JSON.
    let parsed: serde_json::Value =
        serde_json::from_str(&text).expect("stdin report must re-render as JSON");
    assert_eq!(parsed["schemaVersion"], 1);
}

#[test]
fn report_rejects_malformed_stdin_json_with_a_configuration_error() {
    let dir = TempProject::new("report-stdin-malformed");

    let output = run_in_with_stdin(&dir.path, &["report", "-"], b"not json at all");
    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty(), "nothing must render on failure");
    let err = stderr(&output);
    assert!(err.contains("invalid report file"), "stderr: {err}");
}

/// Negative test for the D-06 cap: one byte more than 10 MiB must abort
/// the read with the input-too-large diagnostic instead of being accepted
/// or silently truncated.
#[test]
fn report_aborts_stdin_larger_than_the_10_mib_limit() {
    let dir = TempProject::new("report-stdin-too-large");
    let oversized = vec![b' '; 10 * 1024 * 1024 + 1];

    let output = run_in_with_stdin(&dir.path, &["report", "-"], &oversized);
    assert_eq!(
        output.status.code(),
        Some(2),
        "oversized stdin must exit with a configuration error"
    );
    assert!(
        stdout(&output).is_empty(),
        "an oversized input must not render partial output"
    );
    let err = stderr(&output);
    assert!(
        err.contains("standard input") && err.contains("10 MiB"),
        "stderr must name the stream and the limit, got: {err}"
    );
}

/// The boundary itself: exactly 10 MiB of whitespace around valid JSON is
/// still accepted, proving the cap is a strict "more than" check.
#[test]
fn report_accepts_stdin_of_exactly_the_10_mib_limit() {
    let dir = TempProject::new("report-stdin-at-limit");
    let json = generated_report(&dir);

    let mut padded = json.into_bytes();
    assert!(
        padded.len() < 10 * 1024 * 1024,
        "test payload must have room to pad"
    );
    padded.resize(10 * 1024 * 1024, b' ');

    let output = run_in_with_stdin(&dir.path, &["report", "-"], &padded);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("**Result: PASS**"));
}
