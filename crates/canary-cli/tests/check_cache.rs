mod support;

use support::{
    run_in, stderr, stdout, TempProject, DECODE_FAILURE_BASE64, VALID_STELLAR_VALUE_BASE64,
};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn xdr_fixture_with_id(id: &str, value_base64: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{value_base64}\"\n"
    )
}

fn xdr_fixture(value_base64: &str) -> String {
    xdr_fixture_with_id("p28-xdr-1", value_base64)
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
        &xdr_fixture(DECODE_FAILURE_BASE64),
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
        &xdr_fixture(DECODE_FAILURE_BASE64),
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
        .filter(|name| name.starts_with('v') && name.ends_with(".json"))
        .collect()
}

#[test]
fn a_custom_cache_dir_receives_the_cache_entries() {
    let dir = TempProject::new("cache-dir");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture(VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["check", "--cache-dir", "custom-cache"]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(
        dir.path.join("custom-cache").is_dir(),
        "the custom directory was created"
    );
    assert_eq!(
        entries_in(&dir.path.join("custom-cache")).len(),
        1,
        "the result was written to the custom directory"
    );
    assert!(
        !dir.path.join(".stellar-canary-cache").exists(),
        "the default directory is not touched when --cache-dir is given"
    );
}

#[test]
fn the_cache_falls_back_to_the_default_directory_without_the_flag() {
    let dir = TempProject::new("cache-dir-default");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture(VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(
        dir.path.join(".stellar-canary-cache").is_dir(),
        "without --cache-dir the entries land in the default directory"
    );
}

#[test]
fn an_unusable_cache_dir_is_a_clear_configuration_error() {
    let dir = TempProject::new("cache-dir-bad");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture(VALID_STELLAR_VALUE_BASE64),
    );
    // A regular file where a directory is required: the path cannot be
    // created, and the failure must be reported before any fixture runs.
    dir.write("blocked", "not a directory");

    let output = run_in(&dir.path, &["check", "--cache-dir", "blocked/nested"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "an unusable --cache-dir is a configuration error\n{}\n{}",
        stdout(&output),
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(err.contains("--cache-dir"), "{err}");
    assert!(err.contains("blocked"), "the path is named: {err}");
}

/// On a read-only directory the failure is a permission error specifically.
/// Skipped implicitly when the test runs as root, where the mode bits do not
/// deny the write and `create_dir_all` would succeed.
#[cfg(unix)]
#[test]
fn a_read_only_cache_dir_reports_a_permission_error() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempProject::new("cache-dir-perm");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture(VALID_STELLAR_VALUE_BASE64),
    );

    let readonly = dir.path.join("readonly");
    std::fs::create_dir(&readonly).unwrap();
    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o555)).unwrap();

    // Running as root bypasses the mode bits, so there is nothing to assert.
    if std::fs::create_dir(readonly.join("probe")).is_ok() {
        std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }

    let output = run_in(&dir.path, &["check", "--cache-dir", "readonly/cache"]);
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(2), "{err}");
    assert!(err.contains("--cache-dir"), "{err}");
    assert!(
        err.to_lowercase().contains("permission"),
        "the error names the permission problem: {err}"
    );

    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn entries_in(path: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('v') && name.ends_with(".json"))
        .collect()
}

fn results(report: &serde_json::Value) -> &Vec<serde_json::Value> {
    report["results"]
        .as_array()
        .unwrap_or_else(|| panic!("`results` is an array: {report}"))
}

fn source_of(report: &serde_json::Value, id: &str) -> String {
    results(report)
        .iter()
        .find(|result| result["testId"] == id)
        .unwrap_or_else(|| panic!("no result {id}: {report}"))["source"]
        .as_str()
        .unwrap_or("<absent>")
        .to_string()
}

fn durations_by_id(
    report: &serde_json::Value,
) -> std::collections::BTreeMap<String, serde_json::Value> {
    results(report)
        .iter()
        .map(|result| {
            (
                result["testId"]
                    .as_str()
                    .expect("testId is a string")
                    .to_string(),
                result["durationMs"].clone(),
            )
        })
        .collect()
}

/// Issue #731: a replay must be visible in the terminal, not just in the
/// cache-hit count. The terminal reporter marks a replayed run with the
/// aggregate badge line
/// `N of M results were replayed from the local result cache ...`; the JSON
/// report carries the per-result `source` and keeps each replayed result's
/// recorded `durationMs`, so both the replayed count and the elapsed time a
/// reader sees describe the run that actually happened.
#[test]
fn replayed_fixtures_are_marked_in_the_terminal_output() {
    let dir = TempProject::new("cache-replay-badge");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-pass.toml",
        &xdr_fixture_with_id("p28-xdr-pass", VALID_STELLAR_VALUE_BASE64),
    );
    dir.write(
        "fixtures/p28-xdr-fail.toml",
        &xdr_fixture_with_id("p28-xdr-fail", DECODE_FAILURE_BASE64),
    );

    // First execution: both fixtures run and their results are recorded.
    let first = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(first.status.code(), Some(1), "{}", stderr(&first));
    let first_report: serde_json::Value =
        serde_json::from_str(&stdout(&first)).expect("stdout is the JSON report");
    assert_eq!(source_of(&first_report, "p28-xdr-pass"), "live");
    assert_eq!(source_of(&first_report, "p28-xdr-fail"), "live");

    // Second execution over identical fixtures: both are answered from the
    // cache, and the terminal says so.
    let second = run_in(&dir.path, &["check"]);
    assert_eq!(second.status.code(), Some(1), "{}", stderr(&second));
    let text = stdout(&second);
    assert!(
        text.contains(
            "2 of 2 results were replayed from the local result cache and were not executed in this run. Use --no-cache for a fresh run."
        ),
        "the replay badge must name the replayed and total counts: {text}"
    );
    // The badge describes fixtures that are still listed, including the
    // failing one: a replay must not launder a cached failure into a pass.
    assert!(text.contains("p28-xdr-pass"), "{text}");
    assert!(text.contains("p28-xdr-fail"), "{text}");
    assert!(text.contains("❌ FAIL"), "{text}");
    assert!(text.contains("1/2 applicable checks passed."), "{text}");
    assert!(text.contains("Status: NOT READY"), "{text}");
    assert_eq!(
        cache_entries(&dir).len(),
        2,
        "replaying cached results writes no new entries"
    );

    // Third execution: the JSON report marks every result as replayed and
    // keeps the elapsed time recorded when each one actually ran.
    let third = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(third.status.code(), Some(1), "{}", stderr(&third));
    let third_report: serde_json::Value =
        serde_json::from_str(&stdout(&third)).expect("stdout is the JSON report");
    for result in results(&third_report) {
        assert_eq!(result["source"], "cache", "{result}");
    }
    assert_eq!(
        durations_by_id(&third_report),
        durations_by_id(&first_report),
        "a replayed result reports the duration recorded when it ran"
    );
}
