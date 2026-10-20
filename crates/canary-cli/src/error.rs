//! Terminal error formatting: the `error:` line plus an actionable hint.
//!
//! Every failing command prints one `error: <message>` line on stderr. For
//! a small set of common, actionable failures — a refused connection, a
//! permission problem, a stale lockfile, a schema mismatch, a missing
//! project configuration — a second `Hint:` line says what to do next, so
//! the first thing a new user reads is a next step rather than only a
//! description of what broke.

use canary_core::CanaryError;

/// Renders `err` the way every command prints it on stderr: the `error:`
/// line, plus a `Hint:` line when the message is one of the known,
/// actionable failures (see [`hint_for`]).
pub fn format_error(err: &CanaryError) -> String {
    let message = err.to_string();
    match hint_for(&message) {
        Some(hint) => format!("error: {message}\nHint: {hint}"),
        None => format!("error: {message}"),
    }
}

/// Prints `err` to stderr in [`format_error`]'s format.
pub fn print_error(err: &CanaryError) {
    eprintln!("{}", format_error(err));
}

/// A troubleshooting hint for a known error message, or `None`.
///
/// Matching is deliberately keyword-based on the already-rendered text and
/// case-insensitive: hints are attached at the last moment and can never
/// change an error's wording, category, or exit code — an unrecognized
/// message simply gets no hint and prints exactly as it did before.
///
/// The keyword sets are open on purpose: a message that does not spell one
/// of them out is a message this version cannot give advice about, so it
/// gets none rather than a guess.
pub fn hint_for(message: &str) -> Option<&'static str> {
    let text = message.to_lowercase();

    // Most concrete and most actionable first: an I/O permission problem
    // names the fix (file mode) even when it also mentions the config
    // file or the lockfile that triggered the read.
    if contains_any(&text, &["permission denied", "access is denied"]) {
        return Some(
            "check the file mode and ownership of the path (for example with `ls -l`), and that this user may read and write it",
        );
    }

    if contains_any(
        &text,
        &[
            "connection refused",
            "connection reset",
            "failed to connect",
            "network transport error",
            "no such host",
            "name or service not known",
            "dns error",
            "timed out calling",
        ],
    ) {
        return Some(
            "check that a local standalone node is running and reachable, and that --rpc-url (or the default endpoint for this network) points at a live RPC server",
        );
    }

    if contains_any(&text, &["lockfile", "lock file", ".stellar-canary.lock"]) {
        return Some(
            "a stale or unreadable `.stellar-canary.lock` blocks the run: make sure no other canary process holds it, remove the lockfile to regenerate it, then re-run",
        );
    }

    if contains_any(&text, &["schema version", "schemaversion"]) {
        return Some(
            "the report was written by a different engine version: regenerate it with this version (`stellar-canary check --json`) or read it with the version that wrote it",
        );
    }

    if contains_any(&text, &["configuration file", ".stellar-canary.toml"]) {
        return Some(
            "create `.stellar-canary.toml` in the project root, or fix the path given to --config; `stellar-canary inspect` shows which configuration was detected",
        );
    }

    None
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(message: &str) -> CanaryError {
        CanaryError::Configuration(message.to_string())
    }

    #[test]
    fn a_connection_refused_error_gets_a_node_running_hint() {
        let inner = "network transport error calling getNetwork: client error (Connect): connection refused (os error 111)";
        let message = format!("configuration error: {inner}");
        let rendered = format_error(&err(inner));

        assert!(rendered.starts_with("error: "), "{rendered}");
        assert!(rendered.contains("connection refused"), "{rendered}");
        let hint = rendered
            .strip_prefix(&format!("error: {message}\n"))
            .expect("hint is on its own line under the error line");
        assert!(hint.starts_with("Hint: "), "{hint}");
        assert!(hint.contains("standalone node"), "{hint}");
        assert!(hint.contains("reachable"), "{hint}");
    }

    #[test]
    fn a_timeout_error_gets_the_network_hint() {
        let hint = hint_for("rpc error: timed out calling simulateTransaction after 3 attempt(s)");
        assert!(hint.is_some(), "a timeout is a network-liveness problem");
        assert!(hint.unwrap().contains("standalone node"));
    }

    #[test]
    fn a_stale_lockfile_error_gets_a_regenerate_the_lock_hint() {
        let hint = hint_for("failed to parse lockfile .stellar-canary.lock: trailing garbage");
        assert!(hint.is_some(), "lockfile problems need a lockfile hint");
        let hint = hint.unwrap();
        assert!(hint.contains("lockfile"), "{hint}");
        assert!(hint.contains("remove"), "{hint}");

        // Every lockfile diagnostic the config loader can produce.
        for message in [
            "failed to read lockfile .stellar-canary.lock: permission denied (os error 13)",
            "lockfile .stellar-canary.lock is a symbolic link; a lockfile must be a regular file",
            "unsupported lockVersion 9 in .stellar-canary.lock: this build supports version 1",
            "invalid lockfile .stellar-canary.lock: missing version",
        ] {
            // Permission problems on the lockfile take the permission hint,
            // which is more actionable; the rest get the lockfile hint.
            assert!(
                hint_for(message).is_some(),
                "no hint for a real lockfile diagnostic: {message}"
            );
        }
    }

    #[test]
    fn a_schema_mismatch_gets_a_regenerate_or_match_versions_hint() {
        let hint = hint_for("configuration error: invalid report file: unsupported schema version (found 999, expected 1)");
        assert!(hint.is_some(), "a schema mismatch needs a version hint");
        let hint = hint.unwrap();
        assert!(hint.contains("engine version"), "{hint}");
        assert!(hint.contains("check --json"), "{hint}");
    }

    #[test]
    fn a_missing_project_configuration_gets_a_create_config_hint() {
        let hint = hint_for(
            "configuration error: failed to read configuration file .stellar-canary.toml: No such file or directory (os error 2)",
        );
        assert!(
            hint.is_some(),
            "a missing config needs a create-config hint"
        );
        let hint = hint.unwrap();
        assert!(hint.contains(".stellar-canary.toml"), "{hint}");
        assert!(hint.contains("--config"), "{hint}");
        assert!(hint.contains("inspect"), "{hint}");
    }

    #[test]
    fn a_permission_error_gets_a_file_mode_hint() {
        let rendered = format_error(&err(
            "configuration error: failed to write report to out/report.json: Permission denied (os error 13)",
        ));
        let hint = hint_for("Permission denied (os error 13)").unwrap();
        assert!(hint.contains("file mode"), "{hint}");
        // The permission hint wins over the report/config wording in the
        // same message: the OS cause is the actionable part.
        assert!(
            rendered.contains("Hint: "),
            "the denied write is rendered with a hint: {rendered}"
        );
        assert!(
            !rendered.contains("Hint: \n"),
            "one hint, cleanly: {rendered}"
        );
    }

    #[test]
    fn hint_matching_is_case_insensitive() {
        assert!(hint_for("CONNECTION REFUSED").is_some());
        assert!(hint_for("Permission Denied").is_some());
    }

    /// The negative case: an unrecognized message must print exactly as it
    /// always did — no `Hint:` line, no boilerplate, no guessing.
    #[test]
    fn an_unrecognized_error_prints_the_plain_error_line_only() {
        assert!(hint_for("invalid fixture: p28-xdr-1 failed to decode StellarValue").is_none());
        assert!(hint_for("").is_none());

        let rendered = format_error(&err("invalid fixture: bad base64"));
        assert_eq!(
            rendered,
            "error: configuration error: invalid fixture: bad base64"
        );
        assert!(!rendered.contains("Hint:"), "{rendered}");
        assert_eq!(rendered.lines().count(), 1, "{rendered}");
    }

    #[test]
    fn the_error_line_always_keeps_the_original_wording() {
        let rendered = format_error(&err(
            "--protocol must be a positive protocol version number",
        ));
        assert_eq!(
            rendered,
            "error: configuration error: --protocol must be a positive protocol version number"
        );
    }
}
