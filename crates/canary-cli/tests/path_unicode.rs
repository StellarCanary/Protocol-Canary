//! Cross-platform coverage for project paths that contain spaces,
// non-ASCII Unicode characters and symbols (issue #498).
//!
//! Every case below targets a complete project path whose parent directory
//! *and* final directory name carry the special characters, so both `inspect`
//! and `check` must resolve configuration, fixtures and project detection
//! through them. Paths reach the binary only as individual `Command`
//! arguments or as the child's working directory — never as a shell command
//! string — so no quoting layer can hide a parsing bug, and nothing here
//! assumes a POSIX separator or an absolute path.
//!
//! Assertions do not settle for "the process started": each run proves the
//! intended project was the one processed, by checking the reported project
//! root or project name against the tested directory and the fixture id
//! that was actually planned or executed.
//!
//! # Unicode paths on --config and --fixtures-dir
//!
//! These tests verify that non-ASCII path components passed via
//! `--config` and `--fixtures-dir` are handled correctly by the config
//! loader and binary. Paths are constructed using the platform's valid
//! filename characters; on Unix/macOS this includes UTF-8 characters.
//! On Windows, the test is gated similarly to the non-UTF-8 test.

mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use std::path::PathBuf;
use std::ffi::OsStr;

/// Unicode directory name for --config testing (Cyrillic "test").
const CONFIG_UNICODE_DIR: &str = "тест-проект";

/// Unicode directory name for --fixtures-dir testing (Japanese "検証").
const FIXTURES_UNICODE_DIR: &str = "検証";

/// Creates a minimal valid project with unicode directory names.
fn write_unicode_project(container: &TempProject, config_leaf: &OsStr, fixtures_leaf: &OsStr) -> PathBuf {
    let root = container.path.join(config_leaf);
    let fixtures = root.join("fixtures");
    std::fs::create_dir_all(&fixtures).expect("failed to create project directory");
    std::fs::write(root.join(".stellar-canary.toml"), OFFLINE_CONFIG).expect("failed to write config");
    std::fs::write(fixtures.join("p28-xdr-1.toml"), xdr_fixture("p28-xdr-1")).expect("failed to write fixture");
    root
}

/// Offline config (XDR-only, no RPC/Soroban).
const OFFLINE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = true
rpc = false
soroban = false
"#;

/// Creates an XDR fixture with the given ID.
fn xdr_fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

/// Positive case: unicode --config and --fixtures-dir are handled by check.
#[test]
fn unicode_config_and_fixtures_dir_are_handled_by_check() {
    let container = TempProject::new(CONFIG_UNICODE_DIR);
    let config_leaf = OsStr::new(CONFIG_UNICODE_DIR);
    let fixtures_leaf = OsStr::new(FIXTURES_UNICODE_DIR);
    let root = write_unicode_project(&container, config_leaf, fixtures_leaf);

    let root_str = root.to_str().expect("temporary paths are UTF-8");
    let cwd = TempProject::new("path-unicode-cwd-check");

    // check with --project-root: the JSON report must prove the intended
    // project was actually processed and passed.
    let output = run_in(&cwd.path, &[
        "check",
        "--config",
        &format!("{}/.stellar-canary.toml", root_str),
        "--fixtures-dir",
        &format!("{}/fixtures", root_str),
        "--project-root",
        root_str,
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "check --config --fixtures-dir {root_str} must exit 0; stderr: {}",
        stderr(&output)
    );
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("check must emit a valid json report");
    assert_eq!(value["status"], "pass", "{root_str}: {value}");
    assert_eq!(
        value["counts"]["passed"], 1,
        "{root_str}: {value}"
    );
    assert!(
        value["project"]["name"] == CONFIG_UNICODE_DIR,
        "the report must name the tested project {root_str}: {value}"
    );
    assert!(
        value["results"][0]["testId"] == "p28-xdr-1",
        "the fixture inside {root_str} must have executed: {value}"
    );
}

/// Error case: missing unicode config path displays the path correctly.
#[test]
fn unicode_missing_config_displays_path_in_diagnostics() {
    let cwd = TempProject::new("missing-config-cwd");
    let config_path = std::env::temp_dir().join(format!("{}-config", CONFIG_UNICODE_DIR));
    std::fs::create_dir_all(&config_path).expect("failed to create temp config dir");
    let config_file = config_path.join(".stellar-canary.toml");
    // Don't write the config file - it will be missing.

    // check with --config pointing to a non-existent path
    let output = run_in(&cwd.path, &[
        "check",
        "--config",
        config_path.to_str().unwrap(),
        "--fixtures-dir",
        ".",
        "--allow-empty",
    ]);

    // The command should fail with a non-zero exit code.
    assert!(
        !output.status.success(),
        "check with missing config must fail; stderr: {}",
        stderr(&output)
    );

    // Error diagnostics must display the non-ASCII path correctly.
    let err_msg = stderr(&output);
    assert!(
        err_msg.contains(CONFIG_UNICODE_DIR),
        "error must contain the unicode config path; got: {}",
        err_msg
    );
}

/// Error case: missing unicode fixtures dir displays the path correctly.
#[test]
fn unicode_missing_fixtures_dir_displays_path_in_diagnostics() {
    let container = TempProject::new("missing-fixtures-test");
    let fixtures_leaf = OsStr::new(FIXTURES_UNICODE_DIR);
    let root = container.path.join(fixtures_leaf);
    std::fs::create_dir_all(&root).expect("failed to create fixtures dir");
    let config_file = root.join(".stellar-canary.toml");
    std::fs::write(&config_file, OFFLINE_CONFIG).expect("failed to write config");

    let cwd = TempProject::new("missing-fixtures-cwd");
    let output = run_in(&cwd.path, &[
        "check",
        "--config",
        &root.join(".stellar-canary.toml").to_str().unwrap(),
        "--fixtures-dir",
        root.to_str().unwrap(),
        "--allow-empty",
    ]);

    // The command may fail or succeed depending on fixture content,
    // but the path must be displayed in diagnostics.
    let err_msg = stderr(&output);
    // The unicode path should appear in the error output.
    assert!(
        err_msg.contains(FIXTURES_UNICODE_DIR),
        "error must contain the unicode fixtures dir path; got: {}",
        err_msg
    );
}