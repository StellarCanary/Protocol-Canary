//! Runs the command examples from the documentation site against the CLI
//! (`[E-102]`, issue #511).
//!
//! Every `stellar-canary ...` line in a fenced `bash`/`sh`/`console` block
//! under `docs-site/src` is checked against the binary that was just built:
//! its subcommand path must exist and every long flag it uses must be listed
//! by that subcommand's `--help`. A flag that is renamed or removed from the
//! CLI therefore fails here instead of in a user's terminal.
//!
//! Nothing is executed: the command's own arguments are never run, only
//! `--help` for its subcommand, so the check needs no network, fixtures or
//! workspace.
//!
//! The `--json` shorthand gets its own coverage (issue #747): every
//! documented example that uses it is fed through the real Clap parser
//! (with `--help` appended, which Clap only honours after every earlier
//! argument has parsed), both as written and with `--json` rewritten to
//! `--format json`. The two spellings are then run side by side on a
//! throwaway offline project for `check`, `inspect` and `fixtures`, and
//! their JSON output must be identical.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

/// The binary this package just built.
const BIN: &str = env!("CARGO_BIN_EXE_stellar-canary");

/// Every Markdown file under the documentation site, in a stable order.
fn doc_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{} is readable: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("directory entry is readable").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                out.push(path);
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs-site/src");
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "no documentation pages found at {}",
        root.display()
    );
    files
}

/// The commands in the `bash`, `sh` and `console` blocks of one page.
fn documented_commands(doc: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut fence: Option<&str> = None;
    let mut block: Vec<String> = Vec::new();

    for line in doc.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if let Some(info) = trimmed.strip_prefix("```") {
            match fence {
                None => {
                    fence = Some(info.trim());
                    block.clear();
                }
                Some(info_string) => {
                    if matches!(info_string, "bash" | "sh" | "console" | "shell") {
                        commands.extend(commands_in_block(&block));
                    }
                    fence = None;
                    block.clear();
                }
            }
            continue;
        }
        if fence.is_some() {
            block.push(line.to_string());
        }
    }

    commands
}

/// Joins continuation lines, then keeps the lines that run the CLI.
fn commands_in_block(block: &[String]) -> Vec<String> {
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut pending = false;

    for line in block {
        let text = line.trim();
        if text.starts_with('#') {
            continue;
        }
        // Drop a trailing comment that is not inside the command itself.
        let text = match text.find(" #") {
            Some(at) => text[..at].trim_end(),
            None => text,
        };
        if let Some(without_line_continuation) = text.strip_suffix('\\') {
            current.push_str(without_line_continuation);
            current.push(' ');
            pending = true;
            continue;
        }
        current.push_str(text);
        if pending {
            commands.push(current.trim().to_string());
            current.clear();
            pending = false;
        } else if !current.trim().is_empty() {
            commands.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        commands.push(current.trim().to_string());
    }

    commands
        .into_iter()
        .filter_map(|command| {
            let command = command.strip_prefix("$ ").unwrap_or(&command).trim();
            command
                .starts_with("stellar-canary ")
                .then(|| command.to_string())
        })
        .collect()
}

/// Splits a command line into arguments, honouring single and double quotes.
fn tokenize(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for c in command.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            None => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// The tokens after `stellar-canary` that name a subcommand, stopping at the
/// first flag or argument that is not a plain subcommand name.
fn subcommand_path(tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .skip(1)
        .take_while(|token| {
            !token.starts_with('-')
                && token.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                && token
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
        .cloned()
        .collect()
}

/// The long flags a command uses, without their `=value` part.
fn long_flags(tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .skip(1)
        .filter(|token| token.starts_with("--") && token.as_str() != "--help")
        .map(|token| token.split('=').next().unwrap_or(token).to_string())
        .collect()
}

/// Runs `stellar-canary <subcommand> --help` and returns the flags the
/// command uses that its help does not list.
fn missing_flags(command: &str) -> Vec<String> {
    let tokens = tokenize(command);
    let subcommands = subcommand_path(&tokens);
    let output = Command::new(BIN)
        .args(&subcommands)
        .arg("--help")
        .output()
        .unwrap_or_else(|e| panic!("could not run {BIN}: {e}"));
    let help = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "`{command}`: `stellar-canary {} --help` failed ({}):\n{}",
        subcommands.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    long_flags(&tokens)
        .into_iter()
        .filter(|flag| !help.contains(flag.as_str()))
        .collect()
}

#[test]
fn every_documented_command_example_still_parses() {
    let mut checked = 0;
    let mut failures = Vec::new();

    for path in doc_files() {
        let doc = std::fs::read_to_string(&path).expect("documentation page is readable");
        for command in documented_commands(&doc) {
            checked += 1;
            for flag in missing_flags(&command) {
                failures.push(format!("{}: `{command}` uses {flag}", path.display()));
            }
        }
    }

    assert!(
        checked >= 10,
        "expected at least 10 documented command examples, found {checked}; \
         the extraction moved and this test is no longer looking at the docs"
    );
    assert!(
        failures.is_empty(),
        "documented commands no longer match the CLI:\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_flag_the_cli_does_not_define_is_reported() {
    // The negative side of the check: a documented flag that no subcommand
    // defines must be reported, so a stale example fails the suite.
    let missing = missing_flags("stellar-canary check --definitely-not-a-flag");
    assert_eq!(missing, vec!["--definitely-not-a-flag".to_string()]);
}

// ---------------------------------------------------------------------------
// `--json` shorthand (issue #747)
// ---------------------------------------------------------------------------

/// Subcommands whose `--json` flag is documented as shorthand for
/// `--format json`.
const JSON_SHORTHAND_SUBCOMMANDS: &[&str] = &["check", "inspect", "fixtures"];

/// Tokens that end the `stellar-canary` invocation on a shell line: what
/// follows belongs to a pipe, a redirection or another command.
const SHELL_OPERATORS: &[&str] = &["|", "||", "&&", ";", ">", ">>", "2>", "2>&1", "<"];

/// The arguments of the `stellar-canary` invocation itself, without any
/// pipe, redirection or chained command after it.
fn invocation_tokens(command: &str) -> Vec<String> {
    tokenize(command)
        .into_iter()
        .take_while(|token| !SHELL_OPERATORS.contains(&token.as_str()))
        .collect()
}

/// Every documented command that uses the `--json` shorthand, as
/// `(page, command)` pairs.
fn documented_json_commands() -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    for path in doc_files() {
        let doc = std::fs::read_to_string(&path).expect("documentation page is readable");
        for command in documented_commands(&doc) {
            if invocation_tokens(&command).iter().any(|t| t == "--json") {
                found.push((path.clone(), command));
            }
        }
    }
    found
}

/// Rewrites every `--json` argument to the explicit `--format json` pair.
fn with_explicit_format(tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .flat_map(|token| {
            if token == "--json" {
                vec!["--format".to_string(), "json".to_string()]
            } else {
                vec![token.clone()]
            }
        })
        .collect()
}

/// Runs the real argument parser over `tokens` (which start with
/// `stellar-canary`) without executing the command: `--help` is appended,
/// and Clap only acts on it once every argument before it has parsed, so an
/// unknown flag or a bad value still fails exactly as it would at run time.
fn parse_only(tokens: &[String]) -> std::process::Output {
    Command::new(BIN)
        .args(&tokens[1..])
        .arg("--help")
        .output()
        .unwrap_or_else(|e| panic!("could not run {BIN}: {e}"))
}

#[test]
fn the_docs_contain_json_shorthand_examples_for_each_subcommand_that_documents_it() {
    let commands = documented_json_commands();
    assert!(
        commands.len() >= 5,
        "expected at least 5 documented `--json` examples, found {}; \
         the extraction moved and this test is no longer looking at the docs",
        commands.len()
    );

    let subcommands: std::collections::BTreeSet<String> = commands
        .iter()
        .filter_map(|(_, command)| {
            subcommand_path(&invocation_tokens(command))
                .first()
                .cloned()
        })
        .collect();
    for subcommand in ["check", "inspect"] {
        assert!(
            subcommands.contains(subcommand),
            "no documented `stellar-canary {subcommand} --json` example found"
        );
    }
    for subcommand in &subcommands {
        assert!(
            JSON_SHORTHAND_SUBCOMMANDS.contains(&subcommand.as_str()),
            "the docs use `--json` with `{subcommand}`, which does not define the shorthand"
        );
    }
}

#[test]
fn every_subcommand_with_json_shorthand_documents_it_as_format_json() {
    for subcommand in JSON_SHORTHAND_SUBCOMMANDS {
        let output = Command::new(BIN)
            .args([subcommand, "--help"])
            .output()
            .expect("help runs");
        assert!(output.status.success(), "`{subcommand} --help` failed");
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("--json"), "`{subcommand}` lists no --json");
        assert!(
            help.contains("Shorthand for --format json"),
            "`{subcommand} --json` must be described as shorthand for --format json:\n{help}"
        );
        assert!(
            help.contains("--format <FORMAT>") && help.contains("json"),
            "`{subcommand}` must accept `--format json`:\n{help}"
        );
    }
}

#[test]
fn documented_json_shorthand_commands_parse_without_unrecognized_flag_errors() {
    let mut failures = Vec::new();

    for (path, command) in documented_json_commands() {
        let tokens = invocation_tokens(&command);
        for (spelling, variant) in [
            ("--json", tokens.clone()),
            ("--format json", with_explicit_format(&tokens)),
        ] {
            let output = parse_only(&variant);
            let err = stderr(&output);
            if !output.status.success() || err.contains("unexpected argument") {
                failures.push(format!(
                    "{}: `{command}` ({spelling} spelling) was rejected by the parser \
                     ({}):\n{err}",
                    path.display(),
                    output.status
                ));
            }
        }
        if !missing_flags(&command).is_empty() {
            failures.push(format!(
                "{}: `{command}` uses flags missing from --help: {:?}",
                path.display(),
                missing_flags(&command)
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "documented `--json` examples no longer parse:\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_misspelled_json_shorthand_is_rejected_by_the_parser() {
    // The negative side: a typo of the shorthand must fail parsing as an
    // unrecognized flag rather than being silently ignored.
    let tokens = invocation_tokens("stellar-canary check --jsn --protocol 28 | jq .");
    assert_eq!(
        tokens,
        ["stellar-canary", "check", "--jsn", "--protocol", "28"]
    );

    let output = parse_only(&tokens);
    assert!(!output.status.success(), "--jsn must not parse");
    assert!(
        stderr(&output).contains("unexpected argument '--jsn'"),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(
        missing_flags("stellar-canary check --jsn"),
        vec!["--jsn".to_string()]
    );
}

#[test]
fn shell_operators_end_the_documented_invocation() {
    assert_eq!(
        invocation_tokens("stellar-canary inspect --json | jq -r '.fingerprint.digest'"),
        ["stellar-canary", "inspect", "--json"]
    );
    assert_eq!(
        invocation_tokens("stellar-canary check --json > result.json"),
        ["stellar-canary", "check", "--json"]
    );
    assert_eq!(
        with_explicit_format(&invocation_tokens(
            "stellar-canary check --protocol 28 --json --output result.json"
        )),
        [
            "stellar-canary",
            "check",
            "--protocol",
            "28",
            "--format",
            "json",
            "--output",
            "result.json"
        ]
    );
}

/// An offline project with one passing XDR fixture, so `check`, `inspect`
/// and `fixtures` all have something to report without any network.
fn offline_project(name: &str) -> TempProject {
    let dir = TempProject::new(name);
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n",
    );
    dir.write(
        "Cargo.toml",
        "[package]\nname = \"json-shorthand\"\nversion = \"0.1.0\"\n",
    );
    dir.write(
        "fixtures/xdr/p28-xdr-shorthand.toml",
        &format!(
            "id = \"p28-xdr-shorthand\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\n\
             description = \"json shorthand parity\"\ntype = \"StellarValue\"\n\
             kind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir
}

/// Parses a JSON document and zeroes every `durationMs`, the one field that
/// legitimately differs between two otherwise identical runs.
fn normalized_json(raw: &str, context: &str) -> Value {
    fn scrub(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                for (key, field) in fields.iter_mut() {
                    if key == "durationMs" {
                        *field = Value::from(0);
                    } else {
                        scrub(field);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(scrub),
            _ => {}
        }
    }
    let mut value: Value = serde_json::from_str(raw)
        .unwrap_or_else(|e| panic!("{context} did not print JSON ({e}):\n{raw}"));
    scrub(&mut value);
    value
}

/// Runs `args` once with `--json` and once with `--format json` and returns
/// both normalized documents.
fn run_both_spellings(dir: &TempProject, args: &[&str]) -> (Value, Value) {
    let run = |format: &[&str]| {
        let mut full: Vec<&str> = args.to_vec();
        full.extend_from_slice(format);
        let output = run_in(&dir.path, &full);
        assert!(
            output.status.success(),
            "`stellar-canary {}` failed ({}):\n{}",
            full.join(" "),
            output.status,
            stderr(&output)
        );
        normalized_json(&stdout(&output), &full.join(" "))
    };
    (run(&["--json"]), run(&["--format", "json"]))
}

#[test]
fn check_json_shorthand_matches_explicit_format_json() {
    let dir = offline_project("docs-json-check");
    let (shorthand, explicit) = run_both_spellings(
        &dir,
        &[
            "check",
            "--no-cache",
            "--fixtures-dir",
            "fixtures",
            "--protocol",
            "28",
        ],
    );
    assert_eq!(shorthand, explicit);
    assert_eq!(shorthand["schemaVersion"], 1);
    assert_eq!(shorthand["status"], "pass");
}

#[test]
fn inspect_json_shorthand_matches_explicit_format_json() {
    let dir = offline_project("docs-json-inspect");
    let (shorthand, explicit) = run_both_spellings(
        &dir,
        &["inspect", "--fixtures-dir", "fixtures", "--protocol", "28"],
    );
    assert_eq!(shorthand, explicit);
    assert_eq!(shorthand["inspectVersion"], 1);
}

#[test]
fn fixtures_json_shorthand_matches_explicit_format_json() {
    let dir = offline_project("docs-json-fixtures");
    let (shorthand, explicit) = run_both_spellings(
        &dir,
        &["fixtures", "--fixtures-dir", "fixtures", "--protocol", "28"],
    );
    assert_eq!(shorthand, explicit);
    assert!(
        shorthand.to_string().contains("p28-xdr-shorthand"),
        "fixture inventory must list the fixture: {shorthand}"
    );
}

#[test]
fn json_shorthand_takes_precedence_over_an_explicit_terminal_format() {
    // `--json` is a shorthand that selects JSON outright, so combining it
    // with the default `--format terminal` still renders JSON identical to
    // `--format json`.
    let dir = offline_project("docs-json-precedence");
    let base = [
        "check",
        "--no-cache",
        "--fixtures-dir",
        "fixtures",
        "--protocol",
        "28",
    ];

    let mut combined = base.to_vec();
    combined.extend(["--json", "--format", "terminal"]);
    let output = run_in(&dir.path, &combined);
    assert!(output.status.success(), "{}", stderr(&output));
    let combined = normalized_json(&stdout(&output), "check --json --format terminal");

    let mut explicit = base.to_vec();
    explicit.extend(["--format", "json"]);
    let output = run_in(&dir.path, &explicit);
    assert!(output.status.success(), "{}", stderr(&output));
    let explicit = normalized_json(&stdout(&output), "check --format json");

    assert_eq!(combined, explicit);
}
