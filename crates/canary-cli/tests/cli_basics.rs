mod support;

use support::{run_in, stdout, TempProject};

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
    for subcommand in ["check", "inspect", "fixtures", "report", "version"] {
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
