//! Fixture-level checks for the `[request]` table (CA-01 section 3),
//! through the public parsing path a real fixture file takes.

use std::path::Path;

use canary_fixtures::parse_fixture_str;
use canary_rpc::{RpcFixture, RpcFixtureError};

fn load(body: &str) -> Result<RpcFixture, RpcFixtureError> {
    let text = format!(
        "id = \"p28-rpc-req\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"ledger\"\n\
         description = \"request table\"\n{body}"
    );
    let loaded = parse_fixture_str(&text, Path::new("rpc/req.toml")).unwrap();
    RpcFixture::from_loaded(&loaded)
}

const ASSERT: &str = "\n[[assert]]\nkind = \"field-exists\"\nfield = \"latestLedger\"\n";

#[test]
fn request_on_a_parameterless_method_is_an_invalid_fixture() {
    let err = load(&format!(
        "method = \"get-network\"\n\n[request]\nlimit = 1\n{ASSERT}"
    ))
    .unwrap_err();
    assert!(err.to_string().contains("takes no parameters"), "{err}");
}

#[test]
fn bad_request_for_an_upstream_method_is_reported_before_dispatch() {
    let err = load(&format!(
        "method = \"get-ledger-entries\"\n\n[request]\nkeys = [\"not base64!\"]\n{ASSERT}"
    ))
    .unwrap_err();
    assert!(err.to_string().contains("[request] keys"), "{err}");
}

#[test]
fn unknown_method_is_still_rejected() {
    assert!(load(&format!("method = \"send-transaction\"\n{ASSERT}")).is_err());
}

#[test]
fn fixture_without_request_has_none() {
    let fixture = load(&format!("method = \"get-network\"\n{ASSERT}")).unwrap();
    assert!(fixture.request.is_none());
}
