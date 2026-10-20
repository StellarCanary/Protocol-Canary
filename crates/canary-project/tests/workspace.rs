//! Cargo workspace member detection (CF-07-C): member discovery, the
//! 64-member bound, dependency aggregation and the scan boundaries.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_core::{Capability, ProjectType};
use canary_project::{
    detect, detect_details, scan_cargo_workspace, scan_pnpm_workspace, DiagnosticCode,
    MAX_WORKSPACE_DEPTH, MAX_WORKSPACE_MEMBERS,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "canary-ws-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, text: &str) {
        let path = relative
            .split('/')
            .fold(self.0.clone(), |path, segment| path.join(segment));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Writes `<dir>/Cargo.toml` declaring the given dependencies.
    fn member(&self, dir: &str, dependencies: &[&str]) {
        let mut text =
            String::from("[package]\nname = \"m\"\nversion = \"0.1.0\"\n\n[dependencies]\n");
        for dependency in dependencies {
            text.push_str(&format!("{dependency} = \"1\"\n"));
        }
        self.write(&format!("{dir}/Cargo.toml"), &text);
    }

    fn workspace(&self, members: &[&str]) {
        self.workspace_with(members, &[]);
    }

    fn workspace_with(&self, members: &[&str], exclude: &[&str]) {
        let list = |items: &[&str]| {
            items
                .iter()
                .map(|item| format!("{item:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        self.write(
            "Cargo.toml",
            &format!(
                "[workspace]\nmembers = [{}]\nexclude = [{}]\n",
                list(members),
                list(exclude)
            ),
        );
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn member_paths(scan: &canary_project::WorkspaceScan) -> Vec<&str> {
    scan.members.iter().map(|m| m.path.as_str()).collect()
}

fn codes(scan: &canary_project::WorkspaceScan) -> Vec<&'static str> {
    scan.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn a_virtual_workspace_is_detected_from_its_members() {
    let dir = Scratch::new("virtual");
    dir.workspace(&["contracts/*"]);
    dir.member("contracts/token", &["soroban-sdk"]);
    dir.member("contracts/util", &["serde"]);

    let context = detect(dir.path());
    assert_eq!(context.project_type, ProjectType::Soroban);
    assert!(context.capabilities.contains(&Capability::SorobanContract));
}

#[test]
fn a_virtual_workspace_stays_unknown_when_no_member_has_a_stellar_dependency() {
    let dir = Scratch::new("none");
    dir.workspace(&["crates/*"]);
    dir.member("crates/a", &["serde"]);

    assert_eq!(detect(dir.path()).project_type, ProjectType::Unknown);
}

#[test]
fn stellar_sdk_in_a_member_is_aggregated() {
    let dir = Scratch::new("sdk");
    dir.workspace(&["a", "b"]);
    dir.member("a", &["serde"]);
    dir.member("b", &["stellar-sdk"]);

    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.has_dependency("stellar-sdk"));
    assert!(!scan.has_dependency("soroban-sdk"));
    assert_eq!(scan.dependency_names(), ["serde", "stellar-sdk"]);
    assert_eq!(detect(dir.path()).project_type, ProjectType::StellarSdk);
}

#[test]
fn dependencies_across_members_are_all_reported() {
    let dir = Scratch::new("aggregate");
    dir.workspace(&["a", "b"]);
    dir.member("a", &["soroban-sdk"]);
    dir.member("b", &["stellar-sdk"]);

    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.has_dependency("soroban-sdk") && scan.has_dependency("stellar-sdk"));
    // Soroban outranks the SDK, as for any other project.
    assert_eq!(detect(dir.path()).project_type, ProjectType::Soroban);
}

#[test]
fn globs_resolve_in_byte_order_and_only_to_directories() {
    let dir = Scratch::new("glob");
    dir.workspace(&["crates/*"]);
    dir.member("crates/zeta", &[]);
    dir.member("crates/alpha", &[]);
    dir.write("crates/README.md", "not a member");

    let scan = scan_cargo_workspace(dir.path());
    assert_eq!(member_paths(&scan), ["crates/alpha", "crates/zeta"]);
    assert!(scan.diagnostics.is_empty(), "{:?}", scan.diagnostics);
}

#[test]
fn question_mark_matches_one_character() {
    let dir = Scratch::new("question");
    dir.workspace(&["c?"]);
    dir.member("c1", &[]);
    dir.member("c22", &[]);

    assert_eq!(member_paths(&scan_cargo_workspace(dir.path())), ["c1"]);
}

#[test]
fn exclude_removes_members_by_path_and_glob() {
    let dir = Scratch::new("exclude");
    dir.workspace_with(&["crates/*"], &["crates/skip", "crates/old-*"]);
    for name in ["keep", "skip", "old-a", "old-b"] {
        dir.member(&format!("crates/{name}"), &[]);
    }

    assert_eq!(
        member_paths(&scan_cargo_workspace(dir.path())),
        ["crates/keep"]
    );
}

#[test]
fn members_at_the_depth_limit_are_found_and_deeper_ones_are_reported() {
    let dir = Scratch::new("depth");
    let at_limit = ["d"; MAX_WORKSPACE_DEPTH].join("/");
    let too_deep = ["d"; MAX_WORKSPACE_DEPTH + 1].join("/");
    dir.member(&at_limit, &["soroban-sdk"]);
    dir.workspace(&[&at_limit]);
    assert_eq!(
        member_paths(&scan_cargo_workspace(dir.path())),
        [at_limit.as_str()]
    );

    dir.workspace(&[&too_deep]);
    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.members.is_empty());
    assert_eq!(codes(&scan), ["scan-bound-hit"]);
}

#[test]
fn more_than_64_members_are_truncated_with_a_boundary_warning() {
    let dir = Scratch::new("limit");
    dir.workspace(&["m/*"]);
    for index in 0..MAX_WORKSPACE_MEMBERS + 6 {
        dir.member(&format!("m/c{index:03}"), &["soroban-sdk"]);
    }

    let scan = scan_cargo_workspace(dir.path());
    assert_eq!(scan.members.len(), MAX_WORKSPACE_MEMBERS);
    // Truncation keeps the first members in byte order, deterministically.
    assert_eq!(scan.members[0].path, "m/c000");
    assert_eq!(scan.members[MAX_WORKSPACE_MEMBERS - 1].path, "m/c063");
    let bound: Vec<_> = scan
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::ScanBoundHit)
        .collect();
    assert_eq!(bound.len(), 1);
    assert!(bound[0].detail.contains("64"), "{}", bound[0].detail);
}

#[test]
fn exactly_64_members_are_not_a_boundary_hit() {
    let dir = Scratch::new("exact");
    dir.workspace(&["m/*"]);
    for index in 0..MAX_WORKSPACE_MEMBERS {
        dir.member(&format!("m/c{index:03}"), &[]);
    }

    let scan = scan_cargo_workspace(dir.path());
    assert_eq!(scan.members.len(), MAX_WORKSPACE_MEMBERS);
    assert!(scan.diagnostics.is_empty());
}

#[test]
fn a_missing_literal_member_is_a_diagnostic_not_an_error() {
    let dir = Scratch::new("missing");
    dir.workspace(&["present", "absent"]);
    dir.member("present", &["soroban-sdk"]);

    let scan = scan_cargo_workspace(dir.path());
    assert_eq!(member_paths(&scan), ["present"]);
    assert_eq!(codes(&scan), ["workspace-member-missing"]);
}

#[test]
fn a_member_directory_without_a_manifest_is_reported() {
    let dir = Scratch::new("no-manifest");
    dir.workspace(&["lib"]);
    std::fs::create_dir_all(dir.path().join("lib")).unwrap();

    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.members.is_empty());
    assert_eq!(codes(&scan), ["workspace-member-missing"]);
}

#[test]
fn an_unparseable_member_manifest_does_not_stop_the_scan() {
    let dir = Scratch::new("bad");
    dir.workspace(&["a", "b"]);
    dir.write("a/Cargo.toml", "this is = = not toml");
    dir.member("b", &["soroban-sdk"]);

    let scan = scan_cargo_workspace(dir.path());
    assert_eq!(member_paths(&scan), ["b"]);
    assert_eq!(codes(&scan), ["manifest-unparseable"]);
}

#[test]
fn an_oversized_member_manifest_is_reported_and_not_read() {
    let dir = Scratch::new("big");
    dir.workspace(&["a"]);
    let mut text = String::from("[dependencies]\nsoroban-sdk = \"1\"\n# ");
    text.push_str(&"x".repeat(1024 * 1024));
    dir.write("a/Cargo.toml", &text);

    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.members.is_empty());
    assert_eq!(codes(&scan), ["manifest-too-large"]);
}

#[test]
fn members_cannot_escape_the_root() {
    let outer = Scratch::new("outer");
    outer.member("elsewhere", &["soroban-sdk"]);
    outer.write(
        "project/Cargo.toml",
        "[workspace]\nmembers = [\"../elsewhere\", \"/etc\", \"a\\\\b\"]\n",
    );

    let scan = scan_cargo_workspace(&outer.path().join("project"));
    assert!(scan.members.is_empty());
    assert_eq!(
        codes(&scan),
        [
            "path-outside-root",
            "path-outside-root",
            "path-outside-root"
        ]
    );
}

#[test]
fn unsupported_glob_syntax_is_reported() {
    let dir = Scratch::new("syntax");
    dir.workspace(&["crates/**", "crates/[ab]", "crates/{a,b}"]);
    dir.member("crates/a", &[]);

    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.members.is_empty());
    assert_eq!(codes(&scan), ["unsupported-syntax"; 3]);
}

#[test]
fn excluded_directory_names_are_never_members() {
    let dir = Scratch::new("excluded-names");
    dir.workspace(&["*"]);
    dir.member("target", &["soroban-sdk"]);
    dir.member("node_modules", &["soroban-sdk"]);
    dir.member("real", &[]);

    assert_eq!(member_paths(&scan_cargo_workspace(dir.path())), ["real"]);
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_not_followed() {
    let dir = Scratch::new("symlink");
    dir.workspace(&["crates/*", "link"]);
    dir.member("outside", &["soroban-sdk"]);
    std::fs::create_dir_all(dir.path().join("crates")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("outside"), dir.path().join("crates/escape"))
        .unwrap();
    std::os::unix::fs::symlink(dir.path().join("outside"), dir.path().join("link")).unwrap();

    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.members.is_empty());
    assert_eq!(codes(&scan), ["symlink-skipped", "symlink-skipped"]);
    assert_eq!(detect(dir.path()).project_type, ProjectType::Unknown);
}

#[test]
fn a_manifest_without_workspace_members_has_no_scan() {
    let dir = Scratch::new("plain");
    dir.member(".", &["soroban-sdk"]);

    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.members.is_empty() && scan.diagnostics.is_empty());
    assert_eq!(detect(dir.path()).project_type, ProjectType::Soroban);
}

#[test]
fn results_are_stable_across_runs() {
    let dir = Scratch::new("stable");
    dir.workspace(&["c/*", "extra"]);
    for name in ["q", "b", "m"] {
        dir.member(&format!("c/{name}"), &["serde"]);
    }
    dir.member("extra", &["soroban-sdk"]);

    assert_eq!(
        scan_cargo_workspace(dir.path()),
        scan_cargo_workspace(dir.path())
    );
}

#[test]
fn pnpm_workspaces_detect_members_and_apply_exclusion_patterns() {
    let dir = Scratch::new("pnpm-members");
    dir.write(
        "pnpm-workspace.yaml",
        "packages:\n  - 'packages/*' # include package directories\n  - '!packages/ignored' # exclude generated package\n",
    );
    dir.write(
        "packages/sdk/package.json",
        r#"{"dependencies":{"@stellar/stellar-sdk":"^13"}}"#,
    );
    dir.write(
        "packages/ignored/package.json",
        r#"{"dependencies":{"soroban-sdk":"1"}}"#,
    );

    let scan = scan_pnpm_workspace(dir.path());
    assert_eq!(member_paths(&scan), ["packages/sdk"]);
    assert!(scan.has_dependency("@stellar/stellar-sdk"));
    assert!(!scan.has_dependency("soroban-sdk"));
    assert!(scan.diagnostics.is_empty(), "{:?}", scan.diagnostics);

    let detection = detect_details(dir.path());
    assert_eq!(detection.context.project_type, ProjectType::RpcConsumer);
    assert_eq!(detection.workspace_members, ["packages/sdk"]);
    assert!(detection
        .inspected
        .contains(&"pnpm-workspace.yaml".to_string()));
    assert!(detection
        .inspected
        .contains(&"packages/sdk/package.json".to_string()));
}

#[test]
fn malformed_pnpm_workspace_reports_diagnostic_and_keeps_root_detection() {
    let dir = Scratch::new("pnpm-malformed");
    dir.write("pnpm-workspace.yaml", "packages: [\n  - 'broken'\n");
    dir.write(
        "package.json",
        r#"{"dependencies":{"@stellar/stellar-sdk":"^13"}}"#,
    );

    let detection = detect_details(dir.path());
    assert_eq!(detection.context.project_type, ProjectType::RpcConsumer);
    assert!(detection.workspace_members.is_empty());
    assert!(detection.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ManifestUnparseable
            && diagnostic.detail.contains("pnpm-workspace.yaml")
    }));
}
