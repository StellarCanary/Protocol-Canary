//! The JSON report records which fixture pack produced it (issue #442).
//!
//! Identity is optional, but a run over a real local fixture directory must
//! carry the CF-02 digest, protocol, registry version and source.

mod support;

use support::{run_in, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use serde_json::Value;

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

#[test]
fn check_json_records_the_local_directory_pack_identity() {
    let dir = TempProject::new("check-pack-identity");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/xdr/example.toml", &fixture("p28-xdr-example"));
    let expected = canary_fixtures::pack_digest(&dir.path.join("fixtures")).unwrap();

    let output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        support::stderr(&output)
    );

    let report: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(report["pack"]["packDigest"], expected.digest.as_str());
    assert_eq!(report["pack"]["protocol"], 28);
    assert_eq!(report["pack"]["registryVersion"], 1);
    assert_eq!(report["pack"]["source"], "directory");
    assert_eq!(
        report["pack"]["packDigest"]
            .as_str()
            .expect("digest is a string")
            .len(),
        7 + 64
    );
}

#[test]
fn a_report_without_a_fixture_directory_omits_pack_identity() {
    let dir = TempProject::new("check-pack-identity-absent");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    let output = run_in(&dir.path, &["check", "--json", "--allow-empty"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        support::stderr(&output)
    );

    let report: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert!(
        report.get("pack").is_none(),
        "no fixture directory means not recorded, not an empty pack: {report}"
    );
}
