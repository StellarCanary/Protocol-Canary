//! End-to-end coverage for `--project-root` on `check`, `inspect` and
//! `fixtures` (CF-07 section 2, decision D-08).
//!
//! The rule under test: the project is the current directory, or the
//! directory named by `--project-root`, and nothing else. Every relative
//! input resolves against that directory, and no parent directory is ever
//! searched. Nothing here touches the network.

mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const SOROBAN_MANIFEST: &str =
    "[package]\nname = \"c\"\nversion = \"0.1.0\"\n\n[dependencies]\nsoroban-sdk = \"22\"\n";

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

fn path_arg(dir: &TempProject) -> &str {
    dir.path.to_str().expect("temporary paths are UTF-8")
}

fn dir_name(dir: &TempProject) -> String {
    dir.path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("temporary directory has a name")
        .to_string()
}

#[test]
fn every_command_rejects_a_project_root_that_does_not_exist() {
    let cwd = TempProject::new("root-missing-cwd");
    let missing = cwd.path.join("no-such-directory");
    let missing = missing.to_str().unwrap();

    for command in ["check", "inspect", "fixtures"] {
        let output = run_in(&cwd.path, &[command, "--project-root", missing]);
        assert_eq!(output.status.code(), Some(2), "{command}");
        let text = stderr(&output);
        assert!(text.contains("--project-root"), "{command}: {text}");
        assert!(text.contains("does not exist"), "{command}: {text}");
    }
}

#[test]
fn every_command_rejects_a_project_root_that_is_a_file() {
    let cwd = TempProject::new("root-file-cwd");
    cwd.write("a-file", "not a directory");
    let file = cwd.path.join("a-file");
    let file = file.to_str().unwrap();

    for command in ["check", "inspect", "fixtures"] {
        let output = run_in(&cwd.path, &[command, "--project-root", file]);
        assert_eq!(output.status.code(), Some(2), "{command}");
        let text = stderr(&output);
        assert!(text.contains("is not a directory"), "{command}: {text}");
    }
}

#[test]
fn inspect_reads_the_project_at_the_root_not_the_working_directory() {
    let project = TempProject::new("root-inspect-project");
    project.write("Cargo.toml", SOROBAN_MANIFEST);
    let cwd = TempProject::new("root-inspect-cwd");

    // Without the flag, the empty working directory is the project.
    let without = run_in(&cwd.path, &["inspect"]);
    assert!(stdout(&without).contains("Detected Soroban contract usage: false"));

    let output = run_in(
        &cwd.path,
        &["inspect", "--project-root", path_arg(&project)],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("Project type: soroban"), "{text}");
    assert!(
        text.contains("Detected Soroban contract usage: true"),
        "{text}"
    );

    // The chosen root is shown, so a caller can see which project was read.
    let shown = text
        .lines()
        .find(|line| line.starts_with("Project root: "))
        .expect("inspect prints the project root");
    assert!(shown.ends_with(&dir_name(&project)), "{shown}");
}

#[test]
fn a_manifest_in_a_parent_directory_is_never_picked_up() {
    let parent = TempProject::new("root-parent");
    parent.write("Cargo.toml", SOROBAN_MANIFEST);
    parent.write("child/placeholder.txt", "the child has no manifest");
    let child = parent.path.join("child");

    // From inside the child, with no flag: the parent's manifest is invisible.
    let from_child = run_in(&child, &["inspect"]);
    assert!(from_child.status.success());
    let text = stdout(&from_child);
    assert!(text.contains("Project type: unknown"), "{text}");
    assert!(
        text.contains("Detected Soroban contract usage: false"),
        "{text}"
    );

    // Naming the child explicitly gives the same answer from anywhere.
    let named = run_in(
        &parent.path,
        &["inspect", "--project-root", child.to_str().unwrap()],
    );
    assert!(named.status.success());
    let text = stdout(&named);
    assert!(text.contains("Project type: unknown"), "{text}");
    assert!(
        text.contains("Detected Soroban contract usage: false"),
        "{text}"
    );

    // And naming the parent still works: the flag selects, it never searches.
    let parent_run = run_in(&child, &["inspect", "--project-root", path_arg(&parent)]);
    assert!(stdout(&parent_run).contains("Detected Soroban contract usage: true"));
}

#[test]
fn relative_fixtures_dir_and_config_resolve_against_the_project_root() {
    let project = TempProject::new("root-relative");
    project.write("custom/p28-xdr-1.toml", &xdr_fixture("p28-xdr-rooted"));
    project.write("alt.toml", "version = 1\nprotocol = 27\n");
    let cwd = TempProject::new("root-relative-cwd");

    let fixtures = run_in(
        &cwd.path,
        &[
            "fixtures",
            "--project-root",
            path_arg(&project),
            "--fixtures-dir",
            "custom",
            "--protocol",
            "28",
        ],
    );
    assert!(fixtures.status.success(), "{}", stderr(&fixtures));
    assert!(stdout(&fixtures).contains("p28-xdr-rooted"));

    let inspect = run_in(
        &cwd.path,
        &[
            "inspect",
            "--project-root",
            path_arg(&project),
            "--config",
            "alt.toml",
        ],
    );
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    assert!(stdout(&inspect).contains("Configured protocol: 27"));
}

#[test]
fn check_resolves_config_fixtures_and_output_against_the_project_root() {
    let project = TempProject::new("root-check-project");
    project.write(".stellar-canary.toml", OFFLINE_CONFIG);
    project.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1"));
    let cwd = TempProject::new("root-check-cwd");

    let output = run_in(
        &cwd.path,
        &[
            "check",
            "--project-root",
            path_arg(&project),
            "--json",
            "--output",
            "result.json",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

    let written = project.path.join("result.json");
    assert!(written.is_file(), "--output resolves against the root");
    assert!(
        !cwd.path.join("result.json").exists(),
        "--output must not land in the working directory"
    );

    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&written).unwrap()).expect("valid json");
    assert_eq!(value["counts"]["passed"], 1);
    // The report names the selected project, not the working directory.
    assert_eq!(value["project"]["name"], dir_name(&project));
}

#[test]
fn an_absolute_input_path_is_left_alone_when_a_project_root_is_given() {
    let project = TempProject::new("root-absolute-project");
    project.write(".stellar-canary.toml", OFFLINE_CONFIG);
    let elsewhere = TempProject::new("root-absolute-fixtures");
    elsewhere.write("p28-xdr-1.toml", &xdr_fixture("p28-xdr-absolute"));

    let output = run_in(
        &project.path,
        &[
            "fixtures",
            "--project-root",
            path_arg(&project),
            "--fixtures-dir",
            path_arg(&elsewhere),
            "--protocol",
            "28",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("p28-xdr-absolute"));
}
