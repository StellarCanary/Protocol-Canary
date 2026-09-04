mod support;

use support::{run_in, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

#[test]
fn inspect_reports_unknown_for_an_empty_project() {
    let dir = TempProject::new("inspect-empty");
    let output = run_in(&dir.path, &["inspect"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Project type: unknown"));
    assert!(text.contains("Detected Soroban contract usage: false"));
}

#[test]
fn inspect_reports_fixture_plan_without_network_calls() {
    let dir = TempProject::new("inspect-fixtures");
    dir.write(
        "fixtures/xdr/p27-xdr-1.toml",
        &format!(
            "id = \"p27-xdr-1\"\nprotocol = 27\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir.write(
        "fixtures/xdr/p28-xdr-1.toml",
        &format!(
            "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir.write(
        "fixtures/soroban/p28-soroban-capability.toml",
        "id = \"p28-soroban-capability\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"test\"\ndescription = \"test\"\nrequired_capabilities = [\"soroban-contract\"]\nsource_account = \"GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF\"\ncontract_id = \"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC\"\nfunction = \"name\"\nsequence_number = 1\n\n[expect]\nkind = \"simulation-success\"\n",
    );

    let output = run_in(
        &dir.path,
        &["inspect", "--fixtures-dir", "fixtures", "--protocol", "28"],
    );
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Fixture compatibility plan (offline)"));
    assert!(text.contains("xdr: p28-xdr-1"));
    assert!(text.contains("p27-xdr-1 [xdr]: fixture targets protocol 27"));
    assert!(text.contains("p28-soroban-capability [soroban]: requires a capability"));
}

#[test]
fn inspect_detects_a_soroban_dependency() {
    let dir = TempProject::new("inspect-soroban");
    dir.write(
        "Cargo.toml",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\n\n[dependencies]\nsoroban-sdk = \"22\"\n",
    );
    let output = run_in(&dir.path, &["inspect"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Project type: soroban"));
    assert!(text.contains("Detected Soroban contract usage: true"));
}

#[test]
fn fixtures_command_lists_fixture_ids_grouped_by_surface() {
    let dir = TempProject::new("fixtures-list");
    dir.write(
        "fixtures/xdr/p28-xdr-1.toml",
        &format!(
            "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir.write(
        "fixtures/rpc/p28-rpc-1.toml",
        "id = \"p28-rpc-1\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"test\"\ndescription = \"test\"\nmethod = \"get-network\"\n",
    );

    let output = run_in(&dir.path, &["fixtures", "--protocol", "28"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Protocol 28 fixtures"));
    assert!(text.contains("XDR"));
    assert!(text.contains("p28-xdr-1"));
    assert!(text.contains("RPC"));
    assert!(text.contains("p28-rpc-1"));
}

#[test]
fn fixtures_command_reports_when_none_are_found() {
    let dir = TempProject::new("fixtures-none");
    let output = run_in(&dir.path, &["fixtures", "--protocol", "28"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("no fixtures found"));
}
