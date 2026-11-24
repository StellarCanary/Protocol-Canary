mod support;

use serde_json::Value;
use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn project(prefix: &str) -> TempProject {
    let dir = TempProject::new(prefix);
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &format!(
            "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir
}

/// Creates a real successful cache entry, then changes only its fixture
/// status so the CLI exercises the warning policy through normal cache reads.
fn seed_warning_result(dir: &TempProject) {
    let first = run_in(&dir.path, &["check"]);
    assert_eq!(first.status.code(), Some(0), "{}", stdout(&first));

    let cache_dir = dir.path.join(".stellar-canary-cache");
    let entry_path = std::fs::read_dir(&cache_dir)
        .unwrap()
        .next()
        .expect("check should cache the XDR result")
        .unwrap()
        .path();
    let mut entry: Value = serde_json::from_slice(&std::fs::read(&entry_path).unwrap()).unwrap();
    entry["result"]["status"] = Value::String("warning".into());
    std::fs::write(&entry_path, serde_json::to_vec_pretty(&entry).unwrap()).unwrap();
}

#[test]
fn warnings_exit_zero_without_fail_on_warning() {
    let dir = project("fail-on-warning-default");
    seed_warning_result(&dir);

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("WARNING"), "{}", stdout(&output));
}

#[test]
fn warnings_exit_one_with_fail_on_warning() {
    let dir = project("fail-on-warning-enabled");
    seed_warning_result(&dir);

    let output = run_in(&dir.path, &["check", "--fail-on-warning"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(stdout(&output).contains("WARN"), "{}", stdout(&output));
}

#[test]
fn no_warnings_exit_zero_with_fail_on_warning() {
    let dir = project("fail-on-warning-clean");
    let output = run_in(&dir.path, &["check", "--fail-on-warning"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}
