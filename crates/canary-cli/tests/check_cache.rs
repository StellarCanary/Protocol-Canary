mod support;

use support::{run_in, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn xdr_fixture(value_base64: &str) -> String {
    format!(
        "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{value_base64}\"\n"
    )
}

/// The reproduced defect: a fixture was edited after a passing run and the
/// next run still reported the old result, because the cache key did not
/// include the fixture's contents.
#[test]
fn an_edited_fixture_is_never_answered_from_a_previous_run() {
    let dir = TempProject::new("cache-edit");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture(VALID_STELLAR_VALUE_BASE64),
    );

    let first = run_in(&dir.path, &["check"]);
    assert_eq!(first.status.code(), Some(0), "{}", stdout(&first));
    assert!(
        dir.path.join(".stellar-canary-cache").is_dir(),
        "a result was cached"
    );

    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("not-valid-xdr-bytes!!!"),
    );
    let second = run_in(&dir.path, &["check"]);
    assert_eq!(second.status.code(), Some(1), "{}", stdout(&second));
    assert!(stdout(&second).contains("Status: NOT READY"));

    // Restoring the original text is a different, previously cached state.
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture(VALID_STELLAR_VALUE_BASE64),
    );
    let third = run_in(&dir.path, &["check"]);
    assert_eq!(third.status.code(), Some(0));
}

#[test]
fn an_edited_payload_file_is_never_answered_from_a_previous_run() {
    // Payload files are part of the fixture's identity even though no
    // runner reads them yet: changing one must not reuse a result.
    let dir = TempProject::new("cache-payload");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &format!(
            "{}input_file = \"data.txt\"\n",
            xdr_fixture(VALID_STELLAR_VALUE_BASE64)
        ),
    );
    dir.write("fixtures/data.txt", "first");
    assert_eq!(run_in(&dir.path, &["check"]).status.code(), Some(0));

    let before = cache_entries(&dir);
    dir.write("fixtures/data.txt", "second");
    assert_eq!(run_in(&dir.path, &["check"]).status.code(), Some(0));
    assert_eq!(
        cache_entries(&dir).len(),
        before.len() + 1,
        "the edited payload produced a new cache entry instead of reusing one"
    );
}

#[test]
fn entries_written_by_an_older_layout_are_ignored() {
    let dir = TempProject::new("cache-legacy");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("not-valid-xdr-bytes!!!"),
    );
    // The layout used by 0.1.1: id, protocol, project, endpoint hash and
    // observed protocol in the name, a bare `result` inside. It would say
    // `pass` for a fixture that fails.
    dir.write(
        ".stellar-canary-cache/p28-xdr-1__p28__unknown__0000000000000000__unknown.json",
        r#"{"result":{"test_id":"p28-xdr-1","protocol":28,"surface":"Xdr","status":"Pass","summary":"stale","details":null,"duration_ms":0,"fixture_id":"p28-xdr-1"}}"#,
    );

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
}

fn cache_entries(dir: &TempProject) -> Vec<String> {
    std::fs::read_dir(dir.path.join(".stellar-canary-cache"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("v2-") && name.ends_with(".json"))
        .collect()
}
