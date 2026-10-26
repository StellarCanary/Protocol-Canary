//! `stellar-canary config validate` ([E-106], issue #457): the command reads
//! `.stellar-canary.toml` with the same strict loader `check` uses and
//! reports what it found, without loading a fixture, running a check, or
//! opening a network socket.
//!
//! The tests drive the actual binary in a fresh temporary project, so the
//! exit codes, the stdout settings report and the stderr diagnostics are the
//! ones a contributor sees before committing.
//!
//! Exit-code reading: `0` for a valid configuration (and for the built-in
//! defaults when there is no file), `1` for a file that is not a valid
//! configuration — the answer to the question that was asked — and `2` when
//! validation never started, because the invocation does not resolve or the
//! file named by `--config` cannot be read.

mod support;

use support::{run_in, stderr, stdout, TempProject};

/// The configuration from the getting-started guide, plus the optional keys
/// left unset so the "not set" report is covered by the passing case.
const VALID: &str = "version = 1\nprotocol = 28\n\n[project]\ntype = \"auto\"\n\n[tests]\nxdr = true\nrpc = true\nsoroban = true\n\n[policy]\nwarnings_are_failures = false\n";

#[test]
fn a_valid_configuration_exits_zero_and_reports_its_settings() {
    let dir = TempProject::new("config-validate-valid");
    dir.write(".stellar-canary.toml", VALID);

    let output = run_in(&dir.path, &["config", "validate"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.starts_with("Valid: "), "{text}");
    assert!(text.contains(".stellar-canary.toml"), "{text}");
    assert!(text.contains("version                      = 1"), "{text}");
    assert!(text.contains("protocol                     = 28"), "{text}");
    assert!(
        text.contains("project.type                 = auto"),
        "{text}"
    );
    assert!(
        text.contains("tests                        = xdr=true rpc=true soroban=true"),
        "{text}"
    );
    assert!(
        text.contains("policy.warnings_are_failures = false"),
        "{text}"
    );
}

/// The optional run settings are reported as unset together with the default
/// that then applies, and set values are reported verbatim — a contributor
/// reading the log can tell the two apart.
#[test]
fn the_report_distinguishes_set_values_from_defaults() {
    let dir = TempProject::new("config-validate-settings");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 23\nnetwork = \"futurenet\"\nfixtures_dir = \"ci/fixtures\"\n",
    );

    let text = stdout(&run_in(&dir.path, &["config", "validate"]));
    assert!(text.contains("protocol                     = 23"), "{text}");
    assert!(
        text.contains("network                      = futurenet"),
        "{text}"
    );
    assert!(
        text.contains("fixtures_dir                 = ci/fixtures"),
        "{text}"
    );
    // `rpc_url` is the one key the file leaves unset here.
    assert!(
        text.contains("rpc_url                      = (not set)"),
        "{text}"
    );

    // A file that sets nothing shows every default.
    let bare = TempProject::new("config-validate-bare");
    bare.write(".stellar-canary.toml", "version = 1\nprotocol = 28\n");
    let text = stdout(&run_in(&bare.path, &["config", "validate"]));
    assert!(
        text.contains("network                      = (not set: defaults to testnet)"),
        "{text}"
    );
    assert!(
        text.contains("fixtures_dir                 = (not set: defaults to fixtures)"),
        "{text}"
    );
    assert!(
        text.contains("tests                        = xdr=true rpc=true soroban=true"),
        "{text}"
    );
}

/// The command validates the configuration and nothing else: a project with
/// no fixture directory, no lockfile and no network still reports a valid
/// configuration, and nothing is written.
#[test]
fn validation_runs_no_check_and_writes_nothing() {
    let dir = TempProject::new("config-validate-no-side-effects");
    dir.write(".stellar-canary.toml", VALID);
    let before = std::fs::read(dir.path.join(".stellar-canary.toml")).unwrap();
    let entries_before = std::fs::read_dir(&dir.path).unwrap().count();

    let output = run_in(&dir.path, &["config", "validate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let after = std::fs::read(dir.path.join(".stellar-canary.toml")).unwrap();
    assert_eq!(before, after, "the configuration file must not change");
    assert_eq!(
        std::fs::read_dir(&dir.path).unwrap().count(),
        entries_before,
        "no new file may appear"
    );
    assert!(!dir.path.join(".stellar-canary-cache").exists());
    assert!(!dir.path.join(".stellar-canary.lock").exists());
    // A run that executed checks would have printed a report of results;
    // stdout here is exactly the header plus the eight settings lines.
    let text = stdout(&output);
    assert_eq!(text.lines().count(), 9, "{text}");
    assert!(!text.contains("passed"), "{text}");
    assert!(!text.contains("schemaVersion"), "{text}");
}

/// A misspelled table is the documented negative case: validation fails, and
/// the diagnostic names the invalid table and where it is.
#[test]
fn a_misspelled_table_fails_validation_naming_the_invalid_table() {
    let dir = TempProject::new("config-validate-misspelled-table");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[test]\nxdr = true\n",
    );

    let output = run_in(&dir.path, &["config", "validate"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let text = stderr(&output);
    assert!(text.contains("unknown field `test`"), "{text}");
    assert!(text.contains("line 4"), "the table's line: {text}");
    assert!(text.contains("`tests`"), "the intended name: {text}");
}

/// The same rejection inside a table: a misspelled key names the key, not
/// just the table it sits in.
#[test]
fn a_misspelled_key_inside_a_table_fails_validation() {
    let dir = TempProject::new("config-validate-misspelled-key");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[tests]\nxdrr = true\n",
    );

    let output = run_in(&dir.path, &["config", "validate"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("unknown field `xdrr`"), "{text}");
    assert!(text.contains("line 5"), "{text}");
}

/// A value of the wrong type is reported with its line and column, not as a
/// generic parse failure.
#[test]
fn a_value_of_the_wrong_type_fails_with_a_line_offset() {
    let dir = TempProject::new("config-validate-wrong-type");
    dir.write(".stellar-canary.toml", "version = 1\nprotocol = \"28\"\n");

    let output = run_in(&dir.path, &["config", "validate"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("invalid type"), "{text}");
    assert!(text.contains("line 2"), "{text}");
}

/// A TOML syntax error keeps the parser's own caret snippet, which is what
/// points the contributor at the line to fix.
#[test]
fn a_syntax_error_fails_with_the_offending_line() {
    let dir = TempProject::new("config-validate-syntax");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\nrequired_capabilities = [\n",
    );

    let output = run_in(&dir.path, &["config", "validate"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("TOML parse error"), "{text}");
    assert!(text.contains("line 3"), "{text}");
    assert!(text.contains("required_capabilities"), "{text}");
}

/// Loader rules that a well-typed value can still break are validation
/// failures too, not argument errors.
#[test]
fn an_unsupported_configuration_version_fails_validation() {
    let dir = TempProject::new("config-validate-version");
    dir.write(".stellar-canary.toml", "version = 2\nprotocol = 28\n");

    let output = run_in(&dir.path, &["config", "validate"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(
        text.contains("unsupported configuration version 2"),
        "{text}"
    );
}

#[test]
fn a_protocol_of_zero_fails_validation() {
    let dir = TempProject::new("config-validate-zero-protocol");
    dir.write(".stellar-canary.toml", "version = 1\nprotocol = 0\n");

    let output = run_in(&dir.path, &["config", "validate"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("positive protocol version"),
        "{}",
        stdout(&output)
    );
}

/// Disabling every surface is not a configuration that can check anything.
#[test]
fn a_configuration_with_no_surface_enabled_fails_validation() {
    let dir = TempProject::new("config-validate-no-surface");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[tests]\nxdr = false\nrpc = false\nsoroban = false\n",
    );

    let output = run_in(&dir.path, &["config", "validate"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("at least one of [tests].xdr"), "{text}");
}

/// A project with no configuration file is a project on the built-in
/// defaults, and the defaults are valid — so this is a pass, with the path it
/// looked for printed so a typo in the file name is visible.
#[test]
fn no_configuration_file_is_the_valid_default() {
    let dir = TempProject::new("config-validate-absent");

    let output = run_in(&dir.path, &["config", "validate"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("No configuration file at"), "{text}");
    assert!(text.contains(".stellar-canary.toml"), "{text}");
    assert!(text.contains("defaults"), "{text}");
}

/// Naming a file explicitly changes the contract: the caller pointed at that
/// path, so not being able to read it is a broken invocation (exit code 2),
/// not an answer about the project's configuration.
#[test]
fn an_explicitly_named_missing_file_is_a_configuration_error() {
    let dir = TempProject::new("config-validate-explicit-missing");

    let output = run_in(
        &dir.path,
        &["config", "validate", "--config", "no-such.toml"],
    );
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("failed to read configuration file"), "{text}");
    assert!(text.contains("no-such.toml"), "{text}");
}

/// `--config` is resolved against `--project-root`, like every other input
/// path, so a monorepo member's configuration can be validated from outside.
#[test]
fn a_config_path_is_resolved_against_the_project_root() {
    let dir = TempProject::new("config-validate-member");
    dir.write("member/.stellar-canary.toml", VALID);

    let output = run_in(
        &dir.path,
        &[
            "config",
            "validate",
            "--project-root",
            "member",
            "--config",
            ".stellar-canary.toml",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("member"),
        "the report names the file it read: {}",
        stdout(&output)
    );

    // The same file validated by path alone, without naming a root.
    let output = run_in(
        &dir.path,
        &[
            "config",
            "validate",
            "--config",
            "member/.stellar-canary.toml",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
}

/// An invocation that does not resolve never reaches the file: exit code 2,
/// with the argument named in the message.
#[test]
fn an_unusable_project_root_is_a_configuration_error() {
    let dir = TempProject::new("config-validate-bad-root");
    dir.write(".stellar-canary.toml", VALID);

    let output = run_in(
        &dir.path,
        &["config", "validate", "--project-root", "no-such-root"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("--project-root"),
        "{}",
        stdout(&output)
    );
}

/// `config` is a command group, not a command: without `validate` there is
/// nothing to run, so it is a usage error (exit code 2) that prints the
/// group's help rather than validating a guessed file.
#[test]
fn the_command_group_requires_its_subcommand() {
    let dir = TempProject::new("config-validate-group");
    dir.write(".stellar-canary.toml", VALID);

    let output = run_in(&dir.path, &["config"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("validate"), "{}", stdout(&output));
    // Nothing was printed to stdout: no verdict was reached.
    assert_eq!(stdout(&output), "");
}

/// The loader is the single source of truth: what this command calls valid,
/// `check` reads identically. A file the loader rejects must fail both, with
/// the same diagnostic wording, so `config validate` can never lull a
/// contributor about a configuration `check` would still refuse.
#[test]
fn the_command_and_check_agree_on_an_invalid_configuration() {
    let dir = TempProject::new("config-validate-agrees-with-check");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[polcy]\n",
    );

    let validate = run_in(&dir.path, &["config", "validate"]);
    let check = run_in(&dir.path, &["check", "--offline"]);
    assert_eq!(validate.status.code(), Some(1), "{}", stdout(&validate));
    assert_eq!(check.status.code(), Some(2), "{}", stdout(&check));

    let key = "unknown field `polcy`";
    assert!(stderr(&validate).contains(key), "{}", stderr(&validate));
    assert!(stderr(&check).contains(key), "{}", stderr(&check));
}
