//! Keeps `docs-site/src/exit-codes.md` in step with the exit codes the
//! CLI actually produces (issue #499).
//!
//! The page is the user-facing contract for CI branching, and
//! `canary_core::ExitCode` is the implementation. These tests parse the
//! page and compare it against `ExitCode::ALL`, so:
//!
//! - renumbering a documented code fails the table test below;
//! - adding an `ExitCode` variant fails to compile until it is listed in
//!   `ExitCode::ALL` (see `all_lists_every_exit_code_variant` in
//!   `canary-core`), at which point the table and section tests fail
//!   until the page documents it;
//! - the raw-exit test below catches an exit status that bypasses the
//!   enum entirely inside `crates/canary-cli/src`.
//!
//! The negative scope is deliberate: this checks the CLI process
//! exit-code contract (the `ExitCode` enum and the binary's single
//! `process::exit` boundary), not every unrelated integer or internal
//! error code elsewhere in the workspace.

use std::ffi::OsStr;
use std::path::Path;

use canary_core::ExitCode;

const DOC: &str = include_str!("../../../docs-site/src/exit-codes.md");

/// The variant name as spelled in the documentation.
///
/// Exhaustive on purpose: a new `ExitCode` variant fails to compile
/// here until it is given its documented name.
fn variant_name(code: ExitCode) -> &'static str {
    match code {
        ExitCode::Pass => "Pass",
        ExitCode::CompatibilityFailure => "CompatibilityFailure",
        ExitCode::ConfigurationError => "ConfigurationError",
        ExitCode::ExecutionError => "ExecutionError",
        ExitCode::InvalidFixture => "InvalidFixture",
        ExitCode::InternalError => "InternalError",
    }
}

/// The rows of the page's first Markdown table as `(code, name)` pairs.
///
/// Only the first table (the exit-code contract table) is read; its
/// header and separator rows are dropped by requiring the first cell to
/// parse as an integer, so a row like `| \`Configuration\`, \`Project\` |`
/// from the later mapping table could never be mistaken for a code.
fn documented_table(doc: &str) -> Vec<(i32, String)> {
    let mut rows = Vec::new();
    let mut in_first_table = false;
    for line in doc.lines() {
        if line.starts_with('|') {
            in_first_table = true;
            let mut cells = line.split('|');
            let leading_pipe = cells.next();
            debug_assert_eq!(leading_pipe, Some(""), "row starts with a pipe");
            let code_cell = cells.next().unwrap_or_default().trim();
            let name_cell = cells.next().unwrap_or_default().trim();
            let (Ok(code), true) = (
                code_cell.trim_matches('`').parse::<i32>(),
                name_cell.starts_with('`') && name_cell.ends_with('`') && name_cell.len() > 2,
            ) else {
                continue;
            };
            rows.push((code, name_cell.trim_matches('`').to_string()));
        } else if in_first_table {
            break;
        }
    }
    assert!(!rows.is_empty(), "no exit-code table rows found in the doc");
    rows
}

#[test]
fn the_documented_table_matches_the_exit_code_enum() {
    let expected: Vec<(i32, String)> = ExitCode::ALL
        .iter()
        .map(|&code| (code.code(), variant_name(code).to_string()))
        .collect();

    let documented = documented_table(DOC);
    assert_eq!(
        documented, expected,
        "the exit-code table in docs-site/src/exit-codes.md must list exactly \
         the ExitCode variants from canary-core, in ascending order, with \
         their values and names"
    );
}

#[test]
fn every_exit_code_has_a_documented_section() {
    for &code in ExitCode::ALL {
        let heading = format!("## `{}` — `{}`", code.code(), variant_name(code));
        assert!(
            DOC.lines().any(|line| line.trim_end() == heading),
            "docs-site/src/exit-codes.md is missing the `{heading}` section"
        );
    }
}

/// The CLI must terminate the process through `ExitCode::code()` only.
///
/// Scans `crates/canary-cli/src` for `process::exit(` calls and
/// requires each argument to route through `ExitCode::code()` rather
/// than a raw numeric literal, so no undocumented exit status can be
/// introduced at the binary boundary.
#[test]
fn the_cli_exits_only_through_the_exit_code_enum() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut call_sites = 0;

    let entries = std::fs::read_dir(&src_dir).expect("canary-cli src directory is readable");
    for entry in entries {
        let path = entry.expect("src entry is readable").path();
        if path.extension() != Some(OsStr::new("rs")) {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("source file is readable");
        let mut rest = source.as_str();
        while let Some(at) = rest.find("process::exit(") {
            rest = &rest[at + "process::exit(".len()..];
            let end = rest
                .find(';')
                .expect("a process::exit call ends with a semicolon");
            let argument = &rest[..end];
            assert!(
                argument.contains(".code()"),
                "{}: process::exit must go through ExitCode::code(), found \
                 `process::exit({argument})`",
                path.display()
            );
            assert!(
                !argument.contains(|c: char| c.is_ascii_digit()),
                "{}: process::exit must not receive a raw numeric literal, \
                 found `process::exit({argument})` — use an ExitCode variant",
                path.display()
            );
            call_sites += 1;
        }
    }

    assert!(
        call_sites >= 1,
        "expected at least one process::exit call in crates/canary-cli/src; \
         the binary's exit boundary moved — update this test to match"
    );
}
