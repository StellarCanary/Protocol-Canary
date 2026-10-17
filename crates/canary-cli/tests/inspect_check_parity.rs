//! `inspect`'s plan must equal what `check` executes (issue #485, unit
//! CA-03-D): `inspect` is advertised as a faithful dry run of `check`,
//! so the fixture ids *and their order* that
//! `stellar-canary inspect --format json` reports under `plan.wouldRun`
//! have to be exactly the ids `stellar-canary check --format json`
//! reports under `results`.

mod support;

use support::{run_in, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use serde_json::Value;

/// A fixture as both `inspect` and `check` identify it: its id and its
/// surface. Comparing `Vec<FixtureRef>` values makes an ordering
/// mismatch visible, which is the point of this suite.
type FixtureRef = (String, String);

/// An offline project: XDR only, so `check` runs without a network.
const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn xdr_fixture(id: &str, protocol: u32) -> String {
    format!(
        "id = \"{id}\"\nprotocol = {protocol}\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

/// A project whose plan spans both surfaces that can run offline and all
/// three skip causes, so parity is checked against a plan that is not
/// trivially one fixture.
fn parity_project(name: &str) -> TempProject {
    let dir = TempProject::new(name);
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    // Written in an order that does not match the order the loader
    // returns, so an ordering bug cannot pass by accident.
    dir.write("fixtures/xdr/z-last.toml", &xdr_fixture("p28-xdr-last", 28));
    dir.write(
        "fixtures/xdr/a-first.toml",
        &xdr_fixture("p28-xdr-first", 28),
    );
    dir.write(
        "fixtures/xdr/m-middle.toml",
        &xdr_fixture("p28-xdr-middle", 28),
    );

    // Skipped: targets another protocol.
    dir.write(
        "fixtures/xdr/p27-old.toml",
        &xdr_fixture("p27-xdr-old", 28 - 1),
    );
    // Skipped: requires a capability the project does not have.
    dir.write(
        "fixtures/xdr/needs-capability.toml",
        &format!(
            "{}\nrequired_capabilities = [\"soroban-contract\"]\n",
            xdr_fixture("p28-xdr-needs-capability", 28)
        ),
    );

    dir
}

/// The plan `inspect` reports, as `(id, surface)` pairs in order.
fn inspect_plan(dir: &TempProject) -> (Vec<FixtureRef>, Vec<FixtureRef>, Value) {
    let output = run_in(
        &dir.path,
        &["inspect", "--format", "json", "--fixtures-dir", "fixtures"],
    );
    assert!(
        output.status.success(),
        "inspect failed: {}",
        stdout(&output)
    );
    let document: Value = serde_json::from_str(&stdout(&output)).expect("inspect prints JSON");

    let would_run = document["plan"]["wouldRun"]
        .as_array()
        .expect("plan.wouldRun array")
        .iter()
        .map(|entry| {
            (
                entry["id"].as_str().expect("id").to_string(),
                entry["surface"].as_str().expect("surface").to_string(),
            )
        })
        .collect();
    let skipped = document["plan"]["skipped"]
        .as_array()
        .expect("plan.skipped array")
        .iter()
        .map(|entry| {
            (
                entry["id"].as_str().expect("id").to_string(),
                entry["surface"].as_str().expect("surface").to_string(),
            )
        })
        .collect();
    (would_run, skipped, document)
}

/// The results `check` reports, as `(fixtureId, surface)` pairs in
/// execution order.
fn check_results(dir: &TempProject) -> (Vec<FixtureRef>, Vec<FixtureRef>) {
    let output = run_in(
        &dir.path,
        &["check", "--format", "json", "--fixtures-dir", "fixtures"],
    );
    assert!(output.status.success(), "check failed: {}", stdout(&output));
    let report: Value = serde_json::from_str(&stdout(&output)).expect("check prints JSON");

    let results = report["results"]
        .as_array()
        .expect("results array")
        .iter()
        .map(|entry| {
            (
                entry["fixtureId"].as_str().expect("fixtureId").to_string(),
                entry["surface"].as_str().expect("surface").to_string(),
            )
        })
        .collect();
    let skipped = report["skipped"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    (
                        entry["fixtureId"].as_str().expect("fixtureId").to_string(),
                        entry["surface"].as_str().expect("surface").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    (results, skipped)
}

/// The one assertion this issue exists for: a discrepancy between the
/// plan and the execution must fail loudly, naming both lists.
fn assert_parity(plan: &[FixtureRef], executed: &[FixtureRef]) {
    assert_eq!(
        plan, executed,
        "inspect's plan does not equal what check executed:\n  planned:  {plan:?}\n  executed: {executed:?}"
    );
}

#[test]
fn inspect_plan_equals_what_check_executes_including_order() {
    let dir = parity_project("parity-plan");

    let (would_run, skipped, document) = inspect_plan(&dir);
    assert_eq!(
        would_run,
        vec![
            ("p28-xdr-first".to_string(), "xdr".to_string()),
            ("p28-xdr-middle".to_string(), "xdr".to_string()),
            ("p28-xdr-last".to_string(), "xdr".to_string()),
        ],
        "the plan preserves the loader's path order, the order check executes"
    );
    assert_eq!(document["plan"]["checkWouldRefuse"], false);
    assert_eq!(document["plan"]["refusal"], Value::Null);
    // All three skip causes are represented, not just protocol mismatches.
    let skip_codes: Vec<&str> = document["plan"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["code"].as_str())
        .collect();
    assert!(skip_codes.contains(&"protocol-mismatch"), "{skip_codes:?}");
    assert!(skip_codes.contains(&"missing-capability"), "{skip_codes:?}");
    assert!(!skipped.is_empty());

    let (executed, check_skipped) = check_results(&dir);
    assert_parity(&would_run, &executed);
    assert_eq!(
        skipped, check_skipped,
        "the skipped fixtures must match too, in order"
    );
}

#[test]
fn inspect_and_check_agree_for_a_surface_disabled_in_configuration() {
    let dir = TempProject::new("parity-disabled-surface");
    // RPC fixtures exist but the surface is off, so both commands must
    // skip them for the same reason and in the same order.
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/xdr/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28));
    dir.write(
        "fixtures/rpc/p28-rpc-1.toml",
        "id = \"p28-rpc-1\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"test\"\ndescription = \"test\"\nmethod = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n",
    );

    let (would_run, skipped, document) = inspect_plan(&dir);
    assert_eq!(
        would_run,
        vec![("p28-xdr-1".to_string(), "xdr".to_string())]
    );
    assert_eq!(
        document["plan"]["skipped"][0]["code"], "surface-disabled",
        "an RPC fixture with the surface off is surface-disabled"
    );
    assert!(!skipped.is_empty());

    let (executed, check_skipped) = check_results(&dir);
    assert_parity(&would_run, &executed);
    assert_eq!(skipped, check_skipped);
}

#[test]
fn inspect_reports_an_empty_plan_that_check_would_refuse() {
    let dir = TempProject::new("parity-empty");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    let (would_run, _skipped, document) = inspect_plan(&dir);
    assert!(would_run.is_empty());
    assert_eq!(document["plan"]["checkWouldRefuse"], true);
    assert!(
        document["plan"]["refusal"]
            .as_str()
            .expect("refusal text")
            .contains("no checks ran"),
        "{}",
        document["plan"]["refusal"]
    );
}

#[test]
fn a_discrepancy_between_plan_and_execution_is_reported_with_a_clear_message() {
    let planned = vec![
        ("p28-xdr-first".to_string(), "xdr".to_string()),
        ("p28-xdr-second".to_string(), "xdr".to_string()),
    ];
    let executed = vec![
        ("p28-xdr-first".to_string(), "xdr".to_string()),
        ("p28-xdr-different".to_string(), "xdr".to_string()),
    ];

    let message = std::panic::catch_unwind(|| assert_parity(&planned, &executed))
        .expect_err("a mismatch must fail the assertion")
        .downcast::<String>()
        .expect("assert_eq! panics with a String here")
        .clone();
    assert!(
        message.contains("inspect's plan does not equal what check executed"),
        "{message}"
    );
    assert!(message.contains("p28-xdr-second"), "{message}");
    assert!(message.contains("p28-xdr-different"), "{message}");
}

#[test]
fn order_matters_even_when_the_same_fixtures_are_planned_and_executed() {
    // The same two fixtures in a different order is a parity failure:
    // execution order is part of the contract (CA-03 section 3).
    let planned = vec![
        ("a".to_string(), "xdr".to_string()),
        ("b".to_string(), "xdr".to_string()),
    ];
    let executed = vec![
        ("b".to_string(), "xdr".to_string()),
        ("a".to_string(), "xdr".to_string()),
    ];
    assert!(std::panic::catch_unwind(|| assert_parity(&planned, &executed)).is_err());
}

#[test]
fn the_json_shorthand_selects_the_same_document_as_the_format_flag() {
    let dir = TempProject::new("parity-json-shorthand");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/xdr/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28));

    let flagged = run_in(
        &dir.path,
        &["inspect", "--format", "json", "--fixtures-dir", "fixtures"],
    );
    let shorthand = run_in(
        &dir.path,
        &["inspect", "--json", "--fixtures-dir", "fixtures"],
    );
    assert!(flagged.status.success() && shorthand.status.success());
    assert_eq!(stdout(&flagged), stdout(&shorthand));
}

#[test]
fn the_terminal_format_is_still_the_default() {
    let dir = TempProject::new("parity-terminal-default");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/xdr/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28));

    let defaulted = run_in(&dir.path, &["inspect", "--fixtures-dir", "fixtures"]);
    let explicit = run_in(
        &dir.path,
        &[
            "inspect",
            "--format",
            "terminal",
            "--fixtures-dir",
            "fixtures",
        ],
    );
    assert!(defaulted.status.success() && explicit.status.success());
    assert_eq!(stdout(&defaulted), stdout(&explicit));
    assert!(
        stdout(&defaulted).contains("Fixture compatibility plan (offline)"),
        "{}",
        stdout(&defaulted)
    );
}
