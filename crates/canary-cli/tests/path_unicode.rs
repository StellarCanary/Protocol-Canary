//! Cross-platform coverage for project paths that contain spaces,
//! non-ASCII Unicode characters and symbols (issue #498).
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
//! Invalid UTF-8 paths are covered by a `#[cfg(all(unix, not(target_os =
//! "macos")))]` test at the bottom of this file; see its doc comment for
//! why that case is constructible only on non-Apple Unix.

mod support;

use std::ffi::OsStr;
use std::path::PathBuf;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

/// XDR-only offline configuration so `check` never needs a network.
const OFFLINE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = true
rpc = false
soroban = false
"#;

fn xdr_fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

/// Creates `container/<leaf>` as a minimal valid project: the offline
/// configuration and one passing XDR fixture, the same fixture shape the
/// existing `check` and `inspect` tests use.
///
/// The leaf is an `OsStr` so the platform-gated non-UTF-8 test at the
/// bottom of this file can reuse this helper, and every path is built with
/// `Path::join` rather than string concatenation, so separators are never
/// hardcoded.
fn write_minimal_project(container: &TempProject, leaf: &OsStr) -> PathBuf {
    let root = container.path.join(leaf);
    let fixtures = root.join("fixtures");
    std::fs::create_dir_all(&fixtures).unwrap_or_else(|e| {
        panic!("failed to create project directory {}: {e}", root.display());
    });
    std::fs::write(root.join(".stellar-canary.toml"), OFFLINE_CONFIG).unwrap_or_else(|e| {
        panic!("failed to write the config in {}: {e}", root.display());
    });
    std::fs::write(fixtures.join("p28-xdr-1.toml"), xdr_fixture("p28-xdr-1"))
        .unwrap_or_else(|e| panic!("failed to write the fixture in {}: {e}", root.display()));
    root
}

/// Asserts that `inspect` and `check` both fully process the project at
/// `container/<leaf>` when the directory is targeted two ways: named
/// explicitly with `--project-root` from an unrelated working directory,
/// and as the process's working directory itself.
///
/// Each command must exit successfully *and* name the intended project and
/// fixture in its output — a run that merely started would not.
fn assert_inspect_and_check(container: &TempProject, leaf: &str) {
    let root = write_minimal_project(container, OsStr::new(leaf));
    // `PathBuf` has no `Display` impl; this renders it safely (and lossily,
    // which cannot happen here: these names are valid UTF-8).
    let root_display = root.display();
    // Same convention as project_root.rs: the temporary paths this harness
    // builds are UTF-8 on every supported platform. (The non-UTF-8 case is
    // exercised separately below, with OsStr arguments.)
    let root_str = root.to_str().expect("temporary paths are UTF-8");
    let cwd = TempProject::new("path-unicode-cwd");

    // inspect, directory given explicitly as --project-root.
    let output = run_in(&cwd.path, &["inspect", "--project-root", root_str]);
    assert!(
        output.status.success(),
        "inspect --project-root {root_display} must exit 0; stderr: {}",
        stderr(&output)
    );
    let text = stdout(&output);
    let shown = text
        .lines()
        .find(|line| line.starts_with("Project root: "))
        .expect("inspect prints the project root");
    assert!(
        shown.ends_with(leaf),
        "inspect must report the tested project root {root_display}; got: {shown}"
    );
    assert!(
        text.contains("Configured protocol: 28"),
        "inspect must read the config inside {root_display}; stdout: {text}"
    );
    assert!(
        text.contains("xdr: p28-xdr-1"),
        "inspect must plan the fixture inside {root_display}; stdout: {text}"
    );

    // inspect with the special directory as the working directory: the
    // relative default paths must resolve inside it.
    let output = run_in(&root, &["inspect"]);
    assert!(
        output.status.success(),
        "inspect run inside {root_display} must exit 0; stderr: {}",
        stderr(&output)
    );
    let text = stdout(&output);
    let shown = text
        .lines()
        .find(|line| line.starts_with("Project root: "))
        .expect("inspect prints the project root");
    assert!(
        shown.ends_with(leaf),
        "inspect run inside {root_display} must report that directory; got: {shown}"
    );
    assert!(
        text.contains("xdr: p28-xdr-1"),
        "inspect run inside {root_display} must plan the fixture in it; stdout: {text}"
    );

    // check with --project-root: the JSON report must prove the intended
    // fixture of the intended project actually ran and passed.
    let output = run_in(&cwd.path, &["check", "--project-root", root_str, "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "check --project-root {root_display} must exit 0; stderr: {}",
        stderr(&output)
    );
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("check must emit a valid json report");
    assert_eq!(value["status"], "pass", "{root_display}: {value}");
    assert_eq!(value["counts"]["passed"], 1, "{root_display}: {value}");
    assert_eq!(
        value["project"]["name"], leaf,
        "the report must name the tested project {root_display}: {value}"
    );
    assert_eq!(
        value["results"][0]["testId"], "p28-xdr-1",
        "the fixture inside {root_display} must have executed: {value}"
    );

    // check with the special directory as the working directory, terminal
    // format: the human report must also name the intended project.
    let output = run_in(&root, &["check"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "check run inside {root_display} must exit 0; stderr: {}",
        stderr(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains(&format!("Project: {leaf} (unknown)")),
        "check run inside {root_display} must report that project; stdout: {text}"
    );
    assert!(
        text.contains("1/1 applicable checks passed."),
        "check must execute the fixture inside {root_display}; stdout: {text}"
    );
    assert!(
        text.contains("Status: PASS"),
        "check must pass for {root_display}; stdout: {text}"
    );
}

/// The container's own name carries the same special characters, so the
/// *complete* path — parent directory and project directory — is tested,
/// not just one component.
#[test]
fn paths_with_spaces_are_handled_by_inspect_and_check() {
    let container = TempProject::new("canary Project Directory");
    assert_inspect_and_check(&container, "Project Directory");
}

#[test]
fn cyrillic_paths_are_handled_by_inspect_and_check() {
    let container = TempProject::new("canary Проект");
    assert_inspect_and_check(&container, "Проект");
}

#[test]
fn japanese_paths_are_handled_by_inspect_and_check() {
    let container = TempProject::new("canary プロジェクト");
    assert_inspect_and_check(&container, "プロジェクト");
}

/// `#` is a legal filename character on Linux, macOS and Windows (it is
/// not one of `<>:"/\|?*`), so the symbol case is portable.
#[test]
fn symbol_paths_are_handled_by_inspect_and_check() {
    let container = TempProject::new("canary Project #1");
    assert_inspect_and_check(&container, "Project #1");
}

/// Runs the binary with `args` as individual `OsStr` arguments inside `cwd`.
///
/// `support::run_in` takes `&[&str]`, which cannot express the byte
/// sequences that Unix filesystems allow in path names; this variant can.
/// Like the shared helper it never builds a shell command string.
///
/// Gated exactly like the non-UTF-8 test below — its only caller — so the
/// helper does not become dead code on macOS, where that test cannot run.
#[cfg(all(unix, not(target_os = "macos")))]
fn run_os(cwd: &std::path::Path, args: &[&OsStr]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_stellar-canary"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to execute stellar-canary binary")
}

/// Negative coverage for invalid UTF-8 byte sequences in paths (issue #498),
/// non-Apple Unix only.
///
/// The issue anticipated "a descriptive OsString encoding error". The actual
/// contract is different and this test pins *it*, verified against the
/// current binary: `--project-root` is a `PathBuf` handed straight to
/// `std::fs`/`canonicalize`, process arguments are raw `OsStr`s, and output
/// renders paths with lossy `Path::display`. A non-UTF-8 project root is
/// therefore accepted and fully processed — the unrepresentable bytes
/// appear as U+FFFD rather than as an error. No production change is needed
/// or made; there is no UTF-8 conversion boundary that rejects such a path.
///
/// The gate is `#[cfg(all(unix, not(target_os = "macos")))]` for two
/// platform reasons: Windows paths are wide strings, so an invalid-UTF-8
/// byte sequence cannot be constructed as a Windows path in the first
/// place; and macOS filesystems reject non-UTF-8 names outright — CI
/// showed `create_dir_all` failing with `Illegal byte sequence (os error
/// 92)` — so the fixture directory could not even be created there. Other
/// Unix systems (Linux, the BSDs) treat filenames as arbitrary byte
/// strings except NUL and `/`, which is where this case is constructible.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_non_utf8_project_root_is_processed_and_rendered_lossily_on_unix() {
    use std::os::unix::ffi::OsStrExt;

    let container = TempProject::new("path-non-utf8");
    // 0xFF is never a valid UTF-8 byte, so this directory name is not
    // representable as a Rust &str — only as raw OsStr bytes.
    let root = write_minimal_project(&container, OsStr::from_bytes(b"bad-\xff-dir"));

    // inspect with the non-UTF-8 directory named explicitly: the bytes must
    // survive as a process argument, and the intended project must run.
    let cwd = TempProject::new("path-non-utf8-cwd");
    let output = run_os(
        &cwd.path,
        &[
            OsStr::new("inspect"),
            OsStr::new("--project-root"),
            root.as_os_str(),
        ],
    );
    assert!(
        output.status.success(),
        "inspect --project-root {} must be accepted, not rejected as invalid UTF-8; stderr: {}",
        root.display(),
        stderr(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains("xdr: p28-xdr-1"),
        "the fixture inside {} must be planned; stdout: {text}",
        root.display()
    );
    let shown = text
        .lines()
        .find(|line| line.starts_with("Project root: "))
        .expect("inspect prints the project root");
    assert!(
        shown.contains('\u{FFFD}'),
        "the non-UTF-8 bytes must be rendered lossily instead of failing; got: {shown}"
    );

    // inspect with the non-UTF-8 directory as the working directory.
    let output = run_os(&root, &[OsStr::new("inspect")]);
    assert!(
        output.status.success(),
        "inspect run inside {} must exit 0; stderr: {}",
        root.display(),
        stderr(&output)
    );
    assert!(
        stdout(&output).contains("xdr: p28-xdr-1"),
        "the fixture inside {} must be planned when it is the working directory",
        root.display()
    );

    // check from inside the directory: the fixture must actually execute
    // and pass, and the report names the project lossily.
    let output = run_os(&root, &[OsStr::new("check")]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "check run inside {} must exit 0; stderr: {}",
        root.display(),
        stderr(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains("Project: bad-\u{FFFD}-dir (unknown)"),
        "check must name the tested project lossily; stdout: {text}"
    );
    assert!(
        text.contains("1/1 applicable checks passed."),
        "check must execute the fixture inside {}; stdout: {text}",
        root.display()
    );
}
