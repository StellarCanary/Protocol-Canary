mod support;

use support::{run_in, TempProject};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RPC_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = false
rpc = true
soroban = false
"#;

#[tokio::test]
async fn a_failing_get_network_call_surfaces_the_error() {
    let server = MockServer::start().await;
    // Mock getNetwork returning an HTTP 500 error
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let dir = TempProject::new("check-network-fail");
    dir.write(".stellar-canary.toml", RPC_CONFIG);
    
    // We only need an empty fixture to make it try to run RPC
    dir.write(
        "fixtures/p28-rpc-1.toml",
        &format!(
            "id = \"p28-rpc-1\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"GetEvents\"\nargs_json = \"{{}}\"\n"
        ),
    );

    let server_uri = server.uri();

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--network",
            "testnet",
            "--rpc-url",
            &server_uri,
        ],
    );

    let text = support::stdout(&output);
    assert!(text.contains("protocol not observed"));
    assert!(text.contains("Internal Server Error") || text.contains("500"), "Output was: {}", text);
    
    let json_output = run_in(
        &dir.path,
        &[
            "check",
            "--network",
            "testnet",
            "--rpc-url",
            &server_uri,
            "--json"
        ],
    );

    let json_text = support::stdout(&json_output);
    let value: serde_json::Value = serde_json::from_str(&json_text).expect("valid json");
    
    let error_text = value["network"]["error"].as_str().expect("Network error field should be present and a string");
    assert!(error_text.contains("Internal Server Error") || error_text.contains("500"), "JSON error was: {}", error_text);
}
