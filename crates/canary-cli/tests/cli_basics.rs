mod support;

use support::{run_in, stderr, stdout, TempProject};

#[test]
fn version_prints_the_crate_version_and_exits_zero() {
    let dir = TempProject::new("cli-version");
    let output = run_in(&dir.path, &["version"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_lists_every_subcommand() {
    let dir = TempProject::new("cli-help");
    let output = run_in(&dir.path, &["--help"]);
    assert!(output.status.success());
    let text = stdout(&output);
    for subcommand in [
        "check",
        "inspect",
        "fixtures",
        "report",
        "diff",
        "cache",
        "config",
        "lock",
        "completions",
        "version",
    ] {
        assert!(
            text.contains(subcommand),
            "--help should mention {subcommand:?}:\n{text}"
        );
    }
}

#[test]
fn an_unknown_subcommand_is_rejected_with_a_nonzero_exit_code() {
    let dir = TempProject::new("cli-unknown");
    let output = run_in(&dir.path, &["not-a-real-command"]);
    assert!(!output.status.success());
}

#[test]
fn help_output_flag_consistency_across_all_subcommands() {
    let dir = TempProject::new("cli-subcommand-help");
    let subcommands = [
        "check",
        "diff",
        "report",
        "fixtures",
        "inspect",
        "config",
        "cache",
        "lock",
        "completions",
        "version",
    ];

    for subcommand in subcommands {
        for flag in ["--help", "-h"] {
            let output = run_in(&dir.path, &[subcommand, flag]);
            assert!(
                output.status.success(),
                "subcommand '{subcommand}' with flag '{flag}' failed with exit code {:?}: {}",
                output.status.code(),
                stderr(&output)
            );
            assert_eq!(
                output.status.code(),
                Some(0),
                "subcommand '{subcommand}' with flag '{flag}' must return exit code 0"
            );

            let text = stdout(&output);
            assert!(
                text.contains("Usage:"),
                "subcommand '{subcommand}' with flag '{flag}' help output missing 'Usage:' section:\n{text}"
            );
            assert!(
                text.contains("Options:"),
                "subcommand '{subcommand}' with flag '{flag}' help output missing 'Options:' section:\n{text}"
            );
        }
    }
}
