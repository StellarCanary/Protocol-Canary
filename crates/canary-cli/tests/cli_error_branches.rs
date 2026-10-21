//! Error branches of `inspect`, `fixtures` and `check` (`[E-111]`,
//! issue #505).
//!
//! Every test runs the built binary in a fresh temporary project and
//! checks the exit code from the documented contract
//! (`docs-site/src/exit-codes.md`) and the stderr diagnostic:
//!
//! - `2` for an unusable command line. That covers both argument-parser
//!   usage errors (clap's message, a usage line or `--help` hint, no
//!   `configuration error:` prefix) and configuration problems found at
//!   run time (an `error: configuration error:` line).
//! - `4` for a fixture that cannot be loaded.
//!
//! Exit `1` is reserved for a compatibility failure, so no error branch
//! here may produce it. `check` is given `--offline` wherever it gets past
//! argument parsing, so none of these runs can reach a network.

mod support;

use std::process::Output;

use support::{run_in, stderr, stdout, TempProject};

const COMMANDS: [&str; 3] = ["inspect", "fixtures", "check"];

/// Extra arguments that keep a command away from the network.
fn offline(command: &str) -> &'static [&'static str] {
    if command == "check" {
        &["--offline"]
    } else {
        &[]
    }
}

fn run(project: &TempProject, command: &str, args: &[&str]) -> Output {
    let mut full = vec![command];
    full.extend_from_slice(offline(command));
    full.extend_from_slice(args);
    run_in(&project.path, &full)
}

/// Asserts the binary ran and exited with `code` (not a signal or a launch
/// failure) and that stderr mentions every entry of `needles`. Returns
/// stderr.
fn assert_exit(output: &Output, context: &str, code: i32, needles: &[&str]) -> String {
    let err = stderr(output);
    assert_eq!(
        output.status.code(),
        Some(code),
        "{context}: wrong exit status {:?}\nstderr:\n{err}",
        output.status
    );
    for needle in needles {
        assert!(
            err.contains(needle),
            "{context}: stderr should mention {needle:?}:\n{err}"
        );
    }
    err
}

/// [`assert_exit`] for a command that stopped before doing anything, so
/// it printed nothing on stdout. (`inspect` streams its project section
/// before it loads fixtures, so a fixture error is checked with
/// [`assert_exit`] alone.)
fn assert_fails(output: &Output, context: &str, code: i32, needles: &[&str]) -> String {
    let err = assert_exit(output, context, code, needles);
    assert!(
        stdout(output).is_empty(),
        "{context}: a command that could not start must not print a result:\n{}",
        stdout(output)
    );
    err
}

/// A clap usage error: exit 2, clap's own message, and a pointer to help.
fn assert_usage_error(output: &Output, context: &str, needles: &[&str]) {
    let err = assert_fails(output, context, 2, needles);
    assert!(err.starts_with("error: "), "{context}: {err}");
    assert!(
        !err.contains("configuration error:"),
        "{context}: a usage error is reported by the argument parser: {err}"
    );
    assert!(err.contains("--help"), "{context}: no help hint: {err}");
}

/// A configuration problem found at run time: exit 2 and an
/// `error: configuration error:` line.
fn assert_configuration_error(output: &Output, context: &str, needles: &[&str]) -> String {
    let err = assert_fails(output, context, 2, needles);
    assert!(
        err.contains("error: configuration error:"),
        "{context}: {err}"
    );
    err
}

fn xdr_fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"error branch fixture\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{}\"\n",
        support::VALID_STELLAR_VALUE_BASE64
    )
}

// --- configuration file -----------------------------------------------------

#[test]
fn a_nonexistent_config_file_is_a_configuration_error() {
    let project = TempProject::new("cli-err-missing-config");
    let missing = project.path.join("missing").join("canary.toml");
    let missing = missing.to_str().unwrap();
    for command in COMMANDS {
        let output = run(&project, command, &["--config", missing]);
        let err = assert_configuration_error(
            &output,
            command,
            &["failed to read configuration file", "canary.toml"],
        );
        assert!(err.contains("Hint:"), "{command}: no next step: {err}");
    }
}

#[test]
fn a_malformed_config_value_reports_its_line_and_expected_type() {
    let project = TempProject::new("cli-err-bad-config");
    project.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = \"twenty-eight\"\n",
    );
    for command in COMMANDS {
        let output = run(&project, command, &[]);
        assert_configuration_error(
            &output,
            command,
            &[
                "failed to parse configuration file",
                ".stellar-canary.toml",
                "line 2",
                "expected u32",
            ],
        );
    }
}

#[test]
fn an_unknown_config_key_is_named_with_the_accepted_keys() {
    let project = TempProject::new("cli-err-unknown-config-key");
    project.write("canary.toml", "protocl = 28\n");
    for command in COMMANDS {
        let output = run(&project, command, &["--config", "canary.toml"]);
        assert_configuration_error(&output, command, &["unknown field `protocl`", "`protocol`"]);
    }
}

#[test]
fn config_that_is_not_toml_is_a_configuration_error() {
    let project = TempProject::new("cli-err-config-syntax");
    project.write(".stellar-canary.toml", "[policy\n");
    for command in COMMANDS {
        let output = run(&project, command, &[]);
        assert_configuration_error(&output, command, &["TOML parse error", "line 1"]);
    }
}

// --- project directory ------------------------------------------------------

#[test]
fn an_invalid_project_root_is_a_configuration_error() {
    let project = TempProject::new("cli-err-project-root");
    project.write("not-a-dir", "x");
    let missing = project.path.join("missing");
    let file = project.path.join("not-a-dir");
    for command in COMMANDS {
        let output = run(
            &project,
            command,
            &["--project-root", missing.to_str().unwrap()],
        );
        assert_configuration_error(&output, command, &["--project-root", "does not exist"]);

        let output = run(
            &project,
            command,
            &["--project-root", file.to_str().unwrap()],
        );
        assert_configuration_error(&output, command, &["--project-root", "is not a directory"]);
    }
}

// --- fixtures ---------------------------------------------------------------

#[test]
fn a_malformed_fixture_is_an_invalid_fixture_naming_the_file_and_field() {
    let project = TempProject::new("cli-err-bad-fixture");
    project.write(
        "fixtures/broken.toml",
        "id = \"broken\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\n",
    );
    for command in COMMANDS {
        let output = run(&project, command, &[]);
        assert_exit(
            &output,
            command,
            4,
            &[
                "invalid fixture",
                "broken.toml",
                "missing field `description`",
            ],
        );
    }
}

#[test]
fn an_unknown_fixture_surface_is_an_invalid_fixture() {
    let project = TempProject::new("cli-err-fixture-surface");
    project.write(
        "fixtures/odd.toml",
        "id = \"odd\"\nprotocol = 28\nsurface = \"horizon\"\ncategory = \"test\"\ndescription = \"d\"\n",
    );
    for command in COMMANDS {
        let output = run(&project, command, &[]);
        assert_exit(
            &output,
            command,
            4,
            &["invalid fixture", "odd.toml", "horizon"],
        );
    }
}

#[test]
fn duplicate_fixture_ids_are_an_invalid_fixture() {
    let project = TempProject::new("cli-err-duplicate-id");
    project.write("fixtures/a.toml", &xdr_fixture("same-id"));
    project.write("fixtures/b.toml", &xdr_fixture("same-id"));
    for command in COMMANDS {
        let output = run(&project, command, &[]);
        assert_exit(&output, command, 4, &["duplicate fixture id", "same-id"]);
    }
}

#[test]
fn a_missing_referenced_payload_is_an_invalid_fixture() {
    let project = TempProject::new("cli-err-missing-payload");
    project.write(
        "fixtures/payload.toml",
        &format!(
            "{}input_file = \"data/missing.bin\"\n",
            xdr_fixture("payload")
        ),
    );
    for command in COMMANDS {
        let output = run(&project, command, &[]);
        assert_exit(&output, command, 4, &["invalid fixture", "missing.bin"]);
    }
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_in_the_fixtures_dir_is_an_invalid_fixture() {
    let project = TempProject::new("cli-err-fixture-link");
    project.write("elsewhere/real.toml", &xdr_fixture("linked"));
    std::fs::create_dir_all(project.path.join("fixtures")).unwrap();
    std::os::unix::fs::symlink(
        project.path.join("elsewhere/real.toml"),
        project.path.join("fixtures/link.toml"),
    )
    .unwrap();
    for command in COMMANDS {
        let output = run(&project, command, &[]);
        assert_exit(&output, command, 4, &["invalid fixture", "symbolic link"]);
    }
}

#[test]
fn a_missing_fixtures_dir_fails_check_but_not_the_listing_commands() {
    let project = TempProject::new("cli-err-missing-fixtures-dir");
    let missing = project.path.join("no-fixtures-here");
    let missing = missing.to_str().unwrap();

    let output = run(&project, "check", &["--fixtures-dir", missing]);
    assert_configuration_error(
        &output,
        "check",
        &["no checks ran", "no-fixtures-here", "--allow-empty"],
    );

    // `inspect` and `fixtures` describe what is there; an absent directory
    // is reported, not an error.
    let output = run(&project, "fixtures", &["--fixtures-dir", missing]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("no fixtures found"));
    let output = run(&project, "inspect", &["--fixtures-dir", missing]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}

// --- argument parsing -------------------------------------------------------

#[test]
fn an_unknown_flag_is_a_usage_error_with_the_subcommand_usage() {
    let project = TempProject::new("cli-err-unknown-flag");
    for command in COMMANDS {
        let output = run(&project, command, &["--no-such-flag"]);
        assert_usage_error(
            &output,
            command,
            &[
                "unexpected argument '--no-such-flag'",
                &format!("Usage: stellar-canary {command}"),
            ],
        );
    }
}

#[test]
fn an_unexpected_positional_argument_is_a_usage_error() {
    let project = TempProject::new("cli-err-positional");
    for command in COMMANDS {
        let output = run(&project, command, &["stray"]);
        assert_usage_error(&output, command, &["unexpected argument 'stray'"]);
    }
}

#[test]
fn a_flag_missing_its_value_is_a_usage_error() {
    let project = TempProject::new("cli-err-missing-value");
    for command in COMMANDS {
        for flag in ["--config", "--fixtures-dir", "--project-root", "--protocol"] {
            let output = run(&project, command, &[flag]);
            assert_usage_error(
                &output,
                &format!("{command} {flag}"),
                &["a value is required for", flag],
            );
        }
    }
}

#[test]
fn a_non_numeric_protocol_is_a_usage_error() {
    let project = TempProject::new("cli-err-protocol-type");
    for command in COMMANDS {
        let output = run(&project, command, &["--protocol", "latest"]);
        assert_usage_error(&output, command, &["invalid value 'latest'", "--protocol"]);
    }
}

#[test]
fn protocol_zero_is_a_configuration_error() {
    let project = TempProject::new("cli-err-protocol-zero");
    for command in COMMANDS {
        let output = run(&project, command, &["--protocol", "0"]);
        assert_configuration_error(&output, command, &["--protocol", "positive"]);
    }
}

#[test]
fn an_unknown_format_lists_the_possible_values() {
    let project = TempProject::new("cli-err-format");
    for command in COMMANDS {
        let output = run(&project, command, &["--format", "yaml"]);
        assert_usage_error(
            &output,
            command,
            &[
                "invalid value 'yaml'",
                "--format",
                "possible values",
                "json",
            ],
        );
    }
}

// --- check-only combinations ------------------------------------------------

#[test]
fn check_rejects_no_cache_together_with_a_live_cache_ttl() {
    let project = TempProject::new("cli-err-cache-conflict");
    let output = run(&project, "check", &["--no-cache", "--live-cache-ttl", "60"]);
    assert_usage_error(
        &output,
        "check",
        &["'--no-cache' cannot be used with '--live-cache-ttl"],
    );
}

#[test]
fn check_rejects_a_zero_live_cache_ttl() {
    let project = TempProject::new("cli-err-cache-ttl");
    let output = run(&project, "check", &["--live-cache-ttl", "0"]);
    assert_usage_error(&output, "check", &["invalid value '0'", "--live-cache-ttl"]);
}

#[test]
fn check_rejects_an_unknown_surface() {
    let project = TempProject::new("cli-err-surface");
    let output = run(&project, "check", &["--surface", "horizon"]);
    assert_usage_error(
        &output,
        "check",
        &["invalid value 'horizon'", "possible values", "soroban"],
    );
}

#[test]
fn check_with_a_fixture_filter_that_matches_nothing_is_a_configuration_error() {
    let project = TempProject::new("cli-err-fixture-filter");
    project.write("fixtures/present.toml", &xdr_fixture("present"));
    let output = run(&project, "check", &["--fixture", "absent"]);
    assert_configuration_error(&output, "check", &["no matching fixtures", "\"absent\""]);
}

#[test]
fn check_with_an_unwritable_output_path_is_a_configuration_error() {
    let project = TempProject::new("cli-err-output");
    project.write("fixtures/present.toml", &xdr_fixture("present"));
    let output_path = project.path.join("no-such-dir").join("report.json");
    let output = run(
        &project,
        "check",
        &[
            "--format",
            "json",
            "--output",
            output_path.to_str().unwrap(),
        ],
    );
    // The run itself completed, so a summary may already be on stdout;
    // only the exit code and the error line are asserted here.
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(2), "{err}");
    assert!(
        err.contains("error: configuration error:")
            && err.contains("failed to write report")
            && err.contains("report.json"),
        "{err}"
    );
}

#[test]
fn no_error_branch_reports_a_compatibility_failure() {
    // Exit 1 is evidence of an incompatibility. A command that could not
    // run must never claim it, whatever went wrong.
    let project = TempProject::new("cli-err-never-one");
    project.write("fixtures/broken.toml", "not = [valid");
    let cases: [&[&str]; 4] = [
        &["--no-such-flag"],
        &["--protocol", "0"],
        &["--config", "missing.toml"],
        &[],
    ];
    for command in COMMANDS {
        for args in cases {
            let output = run(&project, command, args);
            assert_ne!(
                output.status.code(),
                Some(1),
                "{command} {args:?}: {}",
                stderr(&output)
            );
            assert!(!output.status.success(), "{command} {args:?}");
        }
    }
}
