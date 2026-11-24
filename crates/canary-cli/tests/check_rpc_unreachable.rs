mod support;

use std::net::TcpListener;
use support::{run_in, stdout, TempProject};

const RPC_FIXTURE: &str = r#"
id = "p28-rpc-get-network"
protocol = 28
surface = "rpc"
category = "network"
description = "getNetwork reports protocol 28 and a passphrase"

method = "get-network"

[[assert]]
kind = "field-exists"
field = "passphrase"
"#;

#[test]
fn check_json_with_unreachable_rpc_url_exits_code_3_and_reports_error() {
    let dir = TempProject::new("check-rpc-unreachable");
    dir.write("fixtures/p28-rpc-get-network.toml", RPC_FIXTURE);

    // Bind and drop an ephemeral TCP listener to obtain a port that is guaranteed closed.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let rpc_url = format!("http://127.0.0.1:{port}");

    let output = run_in(&dir.path, &["check", "--json", "--rpc-url", &rpc_url]);

    assert_eq!(
        output.status.code(),
        Some(3),
        "unreachable RPC should map to exit code 3 (ExecutionError)"
    );

    let stdout_text = stdout(&output);
    let value: serde_json::Value = serde_json::from_str(&stdout_text)
        .expect("stdout must parse as valid JSON conforming to CF-01 report structure");

    // Top-level schema conformance checks
    assert_eq!(value["schemaVersion"], 1);
    assert!(value["toolVersion"].is_string());
    assert_eq!(value["targetProtocol"], 28);
    assert!(value["project"]["name"].is_string());
    assert!(value["project"]["type"].is_string());

    // Acceptance criteria assertions
    assert_eq!(value["status"], "error", "top-level status must be error");
    assert_eq!(value["counts"]["errors"], 1, "counts.errors must be 1");
    assert_eq!(value["counts"]["passed"], 0, "counts.passed must be 0");
    assert_eq!(value["counts"]["total"], 1, "counts.total must be 1");

    let results = value["results"]
        .as_array()
        .expect("results must be an array");
    assert_eq!(results.len(), 1, "results must contain 1 entry");

    let fixture_result = &results[0];
    assert_eq!(fixture_result["testId"], "p28-rpc-get-network");
    assert_eq!(fixture_result["surface"], "rpc");
    assert_eq!(
        fixture_result["status"], "error",
        "fixture result status must be error"
    );

    let summary = fixture_result["summary"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let details = fixture_result["details"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let combined_message = format!("{summary} {details}");

    assert!(
        combined_message.contains("connection refused")
            || combined_message.contains("transport failure")
            || combined_message.contains("transport"),
        "fixture result must reference connection refusal or transport failure in summary/details, got: summary={:?}, details={:?}",
        fixture_result["summary"],
        fixture_result["details"]
    );
}
