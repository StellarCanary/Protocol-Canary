mod support;

use support::{run_in, stdout, TempProject};

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

#[test]
fn report_on_a_malformed_file_exits_with_a_configuration_error() {
    let dir = TempProject::new("report-malformed");
    dir.write("result.json", "not json at all");

    let output = run_in(&dir.path, &["report", "result.json"]);
    assert_eq!(output.status.code(), Some(2));
}
