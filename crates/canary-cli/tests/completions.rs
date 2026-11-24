//! End-to-end tests for `stellar-canary completions`.
//!
//! Every test runs the compiled binary the way a user would, so the
//! assertions cover the real dispatch path: clap parsing, the command
//! definition the script is generated from, and what actually reaches
//! stdout versus stderr.

mod support;

use support::{run_in, stderr, stdout, TempProject};

/// The four shells `completions` accepts, each paired with a recognizable
/// piece of that shell's completion syntax. The assertions check behavior —
/// success, recognizable syntax, and a script derived from the real CLI —
/// rather than a byte-for-byte snapshot of the generated script, which
/// would break on every new subcommand.
const SUPPORTED_SHELLS: &[(&str, &str)] = &[
    ("bash", "complete -F"),
    ("zsh", "#compdef stellar-canary"),
    ("fish", "complete -c stellar-canary"),
    ("powershell", "Register-ArgumentCompleter"),
];

/// Fragments that only appear when the script was generated from the real
/// `stellar-canary` command tree: the binary name, an actual subcommand,
/// and an actual option.
const REAL_CLI_FRAGMENTS: &[&str] = &["stellar-canary", "check", "fixtures-dir"];

#[test]
fn completions_prints_a_script_for_every_supported_shell() {
    let dir = TempProject::new("completions-supported");

    for (shell, syntax_marker) in SUPPORTED_SHELLS {
        let output = run_in(&dir.path, &["completions", shell]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "completions {shell} should succeed: {}",
            stderr(&output)
        );

        let script = stdout(&output);
        assert!(
            !script.is_empty(),
            "{shell}: stdout should carry the completion script"
        );
        assert!(
            script.contains(syntax_marker),
            "{shell}: stdout should contain {syntax_marker:?}:\n{script}"
        );
        for fragment in REAL_CLI_FRAGMENTS {
            assert!(
                script.contains(fragment),
                "{shell}: the script should mention {fragment:?}:\n{script}"
            );
        }

        // stdout must stay usable as a completion-script file: nothing but
        // the script may go there, and a successful run says nothing on
        // stderr either.
        assert!(
            stderr(&output).is_empty(),
            "{shell}: a successful run writes nothing to stderr: {}",
            stderr(&output)
        );
    }
}

#[test]
fn an_unsupported_shell_fails_and_lists_every_supported_shell() {
    let dir = TempProject::new("completions-unsupported");

    let output = run_in(&dir.path, &["completions", "unsupported"]);

    // A bad argument value is a usage error: clap exits 2 on stderr.
    assert_eq!(
        output.status.code(),
        Some(2),
        "an unsupported shell should exit 2: {}",
        stderr(&output)
    );

    let err = stderr(&output);
    assert!(
        err.contains("unsupported"),
        "the error should name the rejected value: {err}"
    );
    // The error lists every accepted input name so the user can correct it.
    for shell in ["bash", "zsh", "fish", "powershell"] {
        assert!(
            err.contains(shell),
            "the error should list the supported shell {shell:?}: {err}"
        );
    }

    // Nothing may reach stdout: no script, no diagnostics.
    assert!(
        stdout(&output).is_empty(),
        "no script is emitted for a rejected shell: {}",
        stdout(&output)
    );
}

#[test]
fn completions_without_a_shell_is_a_usage_error() {
    let dir = TempProject::new("completions-missing-arg");

    let output = run_in(&dir.path, &["completions"]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "a missing shell should exit 2: {}",
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(
        err.contains("SHELL"),
        "the error should name the missing argument: {err}"
    );
    assert!(
        stdout(&output).is_empty(),
        "a usage error must not print a script: {}",
        stdout(&output)
    );
}

/// The shell argument takes exactly one value: a second positional is a
/// usage error, not silently ignored or used as another shell.
#[test]
fn an_extra_argument_after_the_shell_is_rejected() {
    let dir = TempProject::new("completions-extra-arg");

    let output = run_in(&dir.path, &["completions", "bash", "zsh"]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "an extra argument should exit 2: {}",
        stderr(&output)
    );
    assert!(
        !stderr(&output).is_empty(),
        "a usage error is reported on stderr"
    );
    assert!(
        stdout(&output).is_empty(),
        "a rejected invocation must not print a script: {}",
        stdout(&output)
    );
}
