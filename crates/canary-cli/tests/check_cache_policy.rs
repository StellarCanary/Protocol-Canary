//! Result-cache policy and replay provenance, end to end.
//!
//! RPC results come from a mock endpoint that counts the `getLatestLedger`
//! calls it receives, so "was this executed or replayed" is observed rather
//! than inferred from the report.

mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LIVE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = true\nsoroban = false\n";

const RPC_FIXTURE: &str = "id = \"p28-rpc-ledger\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"test\"\ndescription = \"test\"\nmethod = \"get-latest-ledger\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"sequence\"\n";

fn xdr_fixture() -> String {
    format!(
        "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

fn start_mock() -> (tokio::runtime::Runtime, MockServer) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let server = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "passphrase": "Test SDF Network ; September 2015",
                    "protocolVersion": 28,
                    "id": "abc",
                    "sequence": 7
                }
            })))
            .mount(&server)
            .await;
        server
    });
    (rt, server)
}

fn ledger_calls(rt: &tokio::runtime::Runtime, server: &MockServer) -> usize {
    rt.block_on(server.received_requests())
        .unwrap()
        .iter()
        .filter(|r| String::from_utf8_lossy(&r.body).contains("getLatestLedger"))
        .count()
}

fn project() -> TempProject {
    let dir = TempProject::new("cache-policy");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture());
    dir.write("fixtures/p28-rpc-ledger.toml", RPC_FIXTURE);
    dir
}

fn check(
    dir: &TempProject,
    server: &MockServer,
    extra: &[&str],
) -> (Option<i32>, serde_json::Value) {
    let mut args = vec!["check", "--json", "--rpc-url"];
    let uri = server.uri();
    args.push(&uri);
    args.extend_from_slice(extra);
    let output = run_in(&dir.path, &args);
    let report: serde_json::Value = serde_json::from_str(&stdout(&output))
        .unwrap_or_else(|e| panic!("report: {e}\n{}\n{}", stdout(&output), stderr(&output)));
    (output.status.code(), report)
}

fn source_of(report: &serde_json::Value, id: &str) -> String {
    report["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["testId"] == id)
        .unwrap_or_else(|| panic!("no result {id}: {report}"))["source"]
        .as_str()
        .unwrap_or("<absent>")
        .to_string()
}

#[test]
fn live_results_are_executed_every_run_by_default() {
    let (rt, server) = start_mock();
    let dir = project();

    let (code, first) = check(&dir, &server, &[]);
    assert_eq!(code, Some(0));
    let (_, second) = check(&dir, &server, &[]);

    assert_eq!(
        ledger_calls(&rt, &server),
        2,
        "the RPC fixture ran both times"
    );
    assert_eq!(source_of(&first, "p28-rpc-ledger"), "live");
    assert_eq!(source_of(&second, "p28-rpc-ledger"), "live");
}

#[test]
fn offline_xdr_results_are_replayed_and_marked() {
    let (_rt, server) = start_mock();
    let dir = project();

    let (_, first) = check(&dir, &server, &[]);
    let (_, second) = check(&dir, &server, &[]);

    assert_eq!(source_of(&first, "p28-xdr-1"), "live");
    assert_eq!(source_of(&second, "p28-xdr-1"), "cache");
}

#[test]
fn a_live_ttl_replays_a_live_result_and_marks_it() {
    let (rt, server) = start_mock();
    let dir = project();

    let (_, first) = check(&dir, &server, &["--live-cache-ttl", "3600"]);
    let (_, second) = check(&dir, &server, &["--live-cache-ttl", "3600"]);

    assert_eq!(source_of(&first, "p28-rpc-ledger"), "live");
    assert_eq!(source_of(&second, "p28-rpc-ledger"), "cache");
    assert_eq!(
        ledger_calls(&rt, &server),
        1,
        "the second run did not call the endpoint"
    );
}

#[test]
fn turning_the_ttl_off_again_stops_replaying_live_results() {
    let (rt, server) = start_mock();
    let dir = project();

    check(&dir, &server, &["--live-cache-ttl", "3600"]);
    let (_, plain) = check(&dir, &server, &[]);

    assert_eq!(source_of(&plain, "p28-rpc-ledger"), "live");
    assert_eq!(ledger_calls(&rt, &server), 2);
}

#[test]
fn no_cache_runs_everything_fresh_and_stores_nothing() {
    let (_rt, server) = start_mock();
    let dir = project();

    let (_, first) = check(&dir, &server, &["--no-cache"]);
    let (_, second) = check(&dir, &server, &["--no-cache"]);

    assert_eq!(source_of(&first, "p28-xdr-1"), "live");
    assert_eq!(source_of(&second, "p28-xdr-1"), "live");
    assert!(!dir.path.join(".stellar-canary-cache").exists());
}

#[test]
fn no_cache_also_ignores_entries_that_already_exist() {
    let (_rt, server) = start_mock();
    let dir = project();
    check(&dir, &server, &[]);

    let (_, fresh) = check(&dir, &server, &["--no-cache"]);
    assert_eq!(source_of(&fresh, "p28-xdr-1"), "live");
}

#[test]
fn no_cache_and_a_live_ttl_together_are_rejected() {
    let (_rt, server) = start_mock();
    let dir = project();
    let uri = server.uri();
    let output = run_in(
        &dir.path,
        &[
            "check",
            "--rpc-url",
            &uri,
            "--no-cache",
            "--live-cache-ttl",
            "60",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("cannot be used with"));
}

#[test]
fn a_zero_ttl_is_rejected() {
    let (_rt, server) = start_mock();
    let dir = project();
    let uri = server.uri();
    let output = run_in(
        &dir.path,
        &["check", "--rpc-url", &uri, "--live-cache-ttl", "0"],
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_corrupted_cache_entry_is_ignored_and_the_fixture_runs_fresh() {
    let (_rt, server) = start_mock();
    let dir = project();
    check(&dir, &server, &[]);

    for entry in std::fs::read_dir(dir.path.join(".stellar-canary-cache")).unwrap() {
        std::fs::write(entry.unwrap().path(), b"{ truncated").unwrap();
    }
    let (code, report) = check(&dir, &server, &[]);
    assert_eq!(code, Some(0));
    assert_eq!(source_of(&report, "p28-xdr-1"), "live");
}

#[test]
fn the_terminal_report_says_when_results_were_replayed() {
    let (_rt, server) = start_mock();
    let dir = project();
    let uri = server.uri();
    run_in(&dir.path, &["check", "--rpc-url", &uri]);
    let output = run_in(&dir.path, &["check", "--rpc-url", &uri]);
    assert!(stdout(&output).contains("1 of 2 results were replayed from the local result cache"));
}
