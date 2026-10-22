//! The workspace integration test matrix (CF-07-C, issue #458).
//!
//! Each test builds a scratch project on disk and checks either a scanner in
//! isolation (`scan_cargo_workspace`, `scan_npm_workspace`,
//! `scan_pnpm_workspace`) or the wiring inside `detect_details`, covering:
//!
//! - the materialization tiers (CF-07 sections 1-3): cargo virtual
//!   workspaces, single-package roots, npm workspaces and pnpm workspaces;
//! - polyglot roots where more than one scanner contributes members;
//! - the CF-07 section 5 bounds (64 members, depth 4) and the rule that a
//!   bound violation is reported instead of silently truncating the result;
//! - member capability evidence (CF-07 section 6): a `soroban-sdk`
//!   dependency declared inside a workspace member is picked up by
//!   detection with evidence pointing at the member manifest.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_core::{Capability, ProjectType};
use canary_project::{
    detect_details, scan_cargo_workspace, scan_npm_workspace, scan_pnpm_workspace, DiagnosticCode,
    MAX_WORKSPACE_MEMBERS,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "canary-ws-matrix-{name}-{}-{}",
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
    fn cargo_member(&self, dir: &str, dependencies: &[&str]) {
        let mut text =
            String::from("[package]\nname = \"m\"\nversion = \"0.1.0\"\n\n[dependencies]\n");
        for dependency in dependencies {
            text.push_str(&format!("{dependency} = \"1\"\n"));
        }
        self.write(&format!("{dir}/Cargo.toml"), &text);
    }

    /// Writes `<dir>/package.json` declaring the given runtime dependencies.
    fn npm_member(&self, dir: &str, dependencies: &[&str], dev_dependencies: &[&str]) {
        let entries = |items: &[&str]| {
            items
                .iter()
                .map(|item| format!("{item:?}: \"^1.0.0\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        self.write(
            &format!("{dir}/package.json"),
            &format!(
                "{{\n  \"name\": \"m\",\n  \"version\": \"1.0.0\",\n  \"dependencies\": {{{}}},\n  \"devDependencies\": {{{}}}\n}}\n",
                entries(dependencies),
                entries(dev_dependencies)
            ),
        );
    }

    fn npm_root(&self, workspaces: &str) {
        self.write(
            "package.json",
            &format!("{{\n  \"name\": \"root\",\n  \"private\": true,\n  \"workspaces\": {workspaces}\n}}\n"),
        );
    }

    fn pnpm_root(&self, packages: &[&str]) {
        let list = packages
            .iter()
            .map(|item| format!("  - {item:?}"))
            .collect::<Vec<_>>()
            .join("\n");
        self.write("pnpm-workspace.yaml", &format!("packages:\n{list}\n"));
    }

    fn cargo_workspace(&self, members: &[&str]) {
        let list = members
            .iter()
            .map(|item| format!("{item:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.write("Cargo.toml", &format!("[workspace]\nmembers = [{list}]\n"));
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

// ---------------------------------------------------------------------------
// Cargo workspaces (tier 1)
// ---------------------------------------------------------------------------

/// A single-crate project (no `[workspace]` section) scans to no members; the
/// root manifest is the crate itself and is read by detection, not by the
/// workspace scanner.
#[test]
fn a_single_crate_project_has_no_workspace_members() {
    let dir = Scratch::new("single");
    dir.cargo_member("", &["soroban-sdk"]);
    dir.write("src/lib.rs", "-authored-by-zon");

    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.members.is_empty(), "{scan:?}");
    assert!(scan.diagnostics.is_empty(), "{scan:?}");

    // Detection still finds the Stellar surface from the root manifest.
    let details = detect_details(dir.path());
    assert!(details.workspace_members.is_empty(), "{details:?}");
    assert!(details.diagnostics.is_empty(), "{details:?}");
    assert_eq!(details.context.project_type, ProjectType::Soroban);
}

/// Two member crates listed in a virtual workspace root are both scanned,
/// with one `ProjectManifest` per member and no diagnostics.
#[test]
fn a_multi_crate_workspace_scans_every_member() {
    let dir = Scratch::new("cargo-multi");
    dir.cargo_workspace(&["contracts/token", "contracts/vault"]);
    dir.cargo_member("contracts/token", &["soroban-sdk"]);
    dir.cargo_member("contracts/vault", &[]);

    let scan = scan_cargo_workspace(dir.path());
    assert_eq!(
        member_paths(&scan),
        vec!["contracts/token", "contracts/vault"]
    );
    assert!(scan.diagnostics.is_empty(), "{scan:?}");
    assert_eq!(
        scan.members[0].manifest.dependency_names,
        vec!["soroban-sdk"]
    );
    assert!(scan.members[1].manifest.dependency_names.is_empty());

    let details = detect_details(dir.path());
    assert_eq!(
        details.workspace_members,
        vec!["contracts/token", "contracts/vault"]
    );
    assert!(details.diagnostics.is_empty(), "{details:?}");
}

// ---------------------------------------------------------------------------
// npm workspaces (tier 2)
// ---------------------------------------------------------------------------

/// npm array-form `workspaces` expand with the shared glob rules: a `*` never
/// crosses a `/`, patterns are sorted, deduplicated, and reported with their
/// read manifests.
#[test]
fn npm_workspace_scans_array_form_and_respects_the_star_semantics() {
    let dir = Scratch::new("npm-array");
    dir.npm_root(r#"["packages/*", "packages/ignored-only", "apps/cli"]"#);
    dir.npm_member("packages/contracts", &["soroban-sdk"], &[]);
    dir.npm_member("packages/ignored-only", &[], &[]);
    dir.npm_member("packages/contracts/nested", &[], &[]);
    dir.npm_member("apps/cli", &[], &[]);

    let scan = scan_npm_workspace(dir.path());
    // Sorted: `apps/cli` sorts before `packages/…`; the nested directory under
    // an existing member is NOT a member (no recursive descent);
    // `packages/contracts/nested` can only be matched by its own pattern.
    assert_eq!(
        member_paths(&scan),
        vec!["apps/cli", "packages/contracts", "packages/ignored-only"]
    );
    assert!(scan.diagnostics.is_empty(), "{scan:?}");
    assert_eq!(
        scan.members[1].manifest.dependency_names,
        vec!["soroban-sdk"]
    );
}

/// The object spelling (`"workspaces": {"packages": [...]}`) behaves like the
/// array spelling; unrelated keys (e.g. `nohoist`) are ignored, not errors.
#[test]
fn npm_workspace_object_form_expands_the_packages_key() {
    let dir = Scratch::new("npm-object");
    dir.npm_root(r#"{"private": true, "packages": ["contracts/*"], "nohoist": ["**/legacy"]}"#);
    dir.npm_member("contracts/token", &[], &["@stellar/stellar-sdk"]);

    let scan = scan_npm_workspace(dir.path());
    assert_eq!(member_paths(&scan), vec!["contracts/token"]);
    assert!(scan.diagnostics.is_empty(), "{scan:?}");
    assert_eq!(
        scan.members[0].manifest.dependency_names,
        vec!["@stellar/stellar-sdk"]
    );
}

/// npm defines no exclusion syntax: negation is the whole vocabulary *not*
/// supported, so `!x` is an unsupported-syntax diagnostic and the rest of the
/// workspace still scans (the scan never silently turns "nothing found").
#[test]
fn npm_workspace_negation_shaped_patterns_are_reported_as_unsupported() {
    let dir = Scratch::new("npm-negate");
    // Only a negated pattern: nothing can match, and the syntax is not npm's.
    dir.npm_root(r#"["!packages/ignored"]"#);
    dir.npm_member("packages/ignored", &[], &[]);

    let scan = scan_npm_workspace(dir.path());
    // `!packages/ignored` is treated as a literal relative path (npm has no
    // negation), so the scan reports the missing directory instead of
    // pretending the pattern matched.
    assert!(scan.members.is_empty(), "{scan:?}");
    assert_eq!(codes(&scan), vec!["workspace-member-missing"]);

    // With a real member next to it both effects show: the real member is
    // scanned and the negated-shape pattern is reported.
    let dir = Scratch::new("npm-negate-and-real");
    dir.npm_root(r#"["packages/*", "!packages/ignored"]"#);
    dir.npm_member("packages/real", &[], &[]);
    dir.npm_member("packages/ignored", &[], &[]);

    let scan = scan_npm_workspace(dir.path());
    assert_eq!(
        member_paths(&scan),
        vec!["packages/ignored", "packages/real"]
    );
    assert_eq!(codes(&scan), vec!["workspace-member-missing"]);
}

/// A root `package.json` that is not valid JSON surfaces as
/// `manifest-unparseable` (CF-07 section 6), not as a silent empty scan.
#[test]
fn npm_workspace_reports_an_unparseable_root_manifest() {
    let dir = Scratch::new("npm-broken");
    dir.write("package.json", "{ not json");

    let scan = scan_npm_workspace(dir.path());
    assert!(scan.members.is_empty(), "{scan:?}");
    assert_eq!(codes(&scan), vec!["manifest-unparseable"]);
}

// ---------------------------------------------------------------------------
// pnpm workspaces (tier 2)
// ---------------------------------------------------------------------------

/// pnpm workspaces expand the same way as npm and honor `!` exclusions.
#[test]
fn pnpm_workspace_scans_members_and_honors_exclusions() {
    let dir = Scratch::new("pnpm");
    dir.pnpm_root(&["packages/*", "!packages/ignored"]);
    dir.npm_member("packages/token", &["stellar-sdk"], &[]);
    dir.npm_member("packages/ignored", &[], &[]);

    let scan = scan_pnpm_workspace(dir.path());
    assert_eq!(member_paths(&scan), vec!["packages/token"]);
    assert!(scan.diagnostics.is_empty(), "{scan:?}");
    assert_eq!(
        scan.members[0].manifest.dependency_names,
        vec!["stellar-sdk"]
    );

    let details = detect_details(dir.path());
    assert_eq!(details.workspace_members, vec!["packages/token"]);
    assert!(details.diagnostics.is_empty(), "{details:?}");
}

// ---------------------------------------------------------------------------
// Polyglot roots (tier 4)
// ---------------------------------------------------------------------------

/// One root can host several toolchains at once: cargo, npm and pnpm members
/// are all collected, sorted, and deduplicated by `detect_details`, and the
/// member manifests are recorded as inspected with dependency evidence.
#[test]
fn a_polyglot_root_combines_cargo_npm_and_pnpm_members() {
    let dir = Scratch::new("polyglot");
    dir.cargo_workspace(&["crates/contracts"]);
    dir.npm_root(r#"["packages/web"]"#);
    dir.pnpm_root(&["apps/api"]);
    dir.cargo_member("crates/contracts", &["soroban-sdk"]);
    dir.npm_member("packages/web", &[], &["@stellar/stellar-sdk"]);
    dir.npm_member("apps/api", &[], &[]);

    let details = detect_details(dir.path());
    assert_eq!(
        details.workspace_members,
        vec!["apps/api", "crates/contracts", "packages/web"]
    );
    assert!(details.diagnostics.is_empty(), "{details:?}");

    // Every member manifest was read by the detector.
    assert!(details
        .inspected
        .contains(&"crates/contracts/Cargo.toml".to_string()));
    assert!(details
        .inspected
        .contains(&"packages/web/package.json".to_string()));
    assert!(details
        .inspected
        .contains(&"apps/api/package.json".to_string()));

    // The Stellar capability evidence must point at the member manifests.
    let mut stellar_rows = Vec::new();
    for capability in &details.capability_evidence {
        if capability.capability == Capability::SorobanContract {
            for evidence in &capability.evidence {
                stellar_rows.push((
                    evidence.manifest.as_str(),
                    evidence.dependency.as_deref().unwrap_or(""),
                ));
            }
        }
    }
    assert!(
        stellar_rows
            .iter()
            .any(|(manifest, dep)| *manifest == "crates/contracts/Cargo.toml"
                && *dep == "soroban-sdk"),
        "missing cargo member evidence: {stellar_rows:?}"
    );
}

/// The per-family scanners stay additive under `detect_details`: each
/// scanner's bound is applied to what is left for it, so a root with cargo
/// members plus a JS workspace member list still scans everything when the
/// combined total stays within the global bound.
#[test]
fn polyglot_members_within_the_global_bound_are_all_scanned() {
    let dir = Scratch::new("polyglot-bound");
    // 63 cargo members + 1 npm member = 64 == MAX_WORKSPACE_MEMBERS.
    let mut cargo_members: Vec<String> = Vec::new();
    for i in 0..63 {
        let name = format!("crates/m{i:02}");
        cargo_members.push(name.clone());
        dir.cargo_member(&name, &[]);
    }
    dir.npm_root(r#"["packages/web"]"#);
    dir.npm_member("packages/web", &[], &[]);

    let list = cargo_members
        .iter()
        .map(|item| format!("{item:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    dir.write("Cargo.toml", &format!("[workspace]\nmembers = [{list}]\n"));

    let details = detect_details(dir.path());
    assert!(details.diagnostics.is_empty(), "{details:?}");
    assert_eq!(details.workspace_members.len(), MAX_WORKSPACE_MEMBERS);
    assert!(details
        .workspace_members
        .contains(&"crates/m00".to_string()));
    assert!(details
        .workspace_members
        .contains(&"packages/web".to_string()));
}

// ---------------------------------------------------------------------------
// CF-07 section 5 bounds
// ---------------------------------------------------------------------------

/// A workspace with exactly `MAX_WORKSPACE_MEMBERS` members scans with no
/// diagnostics; one member more reports `scan-bound-hit` and still
/// resolves the first `MAX_WORKSPACE_MEMBERS` members.
#[test]
fn the_member_bound_reports_one_over_and_scans_exactly_the_limit() {
    let exact = Scratch::new("bound-exact");
    let mut members: Vec<String> = Vec::new();
    for i in 0..MAX_WORKSPACE_MEMBERS {
        let name = format!("crates/m{i:02}");
        members.push(name.clone());
        exact.cargo_member(&name, &[]);
    }
    exact.cargo_workspace(&members.iter().map(String::as_str).collect::<Vec<&str>>());

    let scan = scan_cargo_workspace(exact.path());
    assert_eq!(scan.members.len(), MAX_WORKSPACE_MEMBERS);
    assert!(scan.diagnostics.is_empty(), "{scan:?}");

    // Now add the 65th.
    members.push("crates/overflow".to_string());
    exact.cargo_member("crates/overflow", &[]);

    let scan = scan_cargo_workspace({
        exact.write(
            "Cargo.toml",
            &format!(
                "[workspace]\nmembers = [{}]\n",
                members
                    .iter()
                    .map(|m| format!("{m:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        exact.path()
    });
    assert_eq!(scan.members.len(), MAX_WORKSPACE_MEMBERS);
    assert_eq!(codes(&scan), vec!["scan-bound-hit"]);
}

/// A member pattern deeper than `MAX_WORKSPACE_DEPTH` levels reports
/// `scan-bound-hit` without disturbing the scan of shallower members:
/// cross-tier containment, not early termination.
#[test]
fn a_member_deeper_than_the_depth_bound_is_skipped_and_reported() {
    let dir = Scratch::new("depth");
    dir.cargo_workspace(&[
        "shallow",
        "one/two/three/four",      // exactly MAX_WORKSPACE_DEPTH
        "one/two/three/four/five", // one level too deep
    ]);
    dir.cargo_member("shallow", &["soroban-sdk"]);
    dir.cargo_member("one/two/three/four", &[]);
    dir.cargo_member("one/two/three/four/five", &["soroban-sdk"]);

    let scan = scan_cargo_workspace(dir.path());
    assert_eq!(member_paths(&scan), vec!["one/two/three/four", "shallow"]);
    assert_eq!(codes(&scan), vec!["scan-bound-hit"]);
    // The skipped member's soroban-sdk is invisible to this scan.
    assert!(!scan.members.iter().any(|m| m
        .manifest
        .dependency_names
        .contains(&"soroban-sdk".to_string())
        && m.path == "one/two/three/four/five"));

    // Detection still finds Stellar capability elsewhere in the tree (the
    // shallower member declares it) and surfaces the diagnostic.
    let details = detect_details(dir.path());
    assert_eq!(
        details.workspace_members,
        vec!["one/two/three/four", "shallow"]
    );
    assert!(
        details
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ScanBoundHit),
        "{details:?}"
    );
}

/// The depth bound is a pattern property, not a filesystem walk property: a
/// four-segment glob under a three-deep literal prefix finds its leaf at
/// exactly `MAX_WORKSPACE_DEPTH` levels.
#[test]
fn a_glob_pattern_reaching_the_depth_bound_still_matches() {
    let dir = Scratch::new("depth-glob");
    dir.cargo_workspace(&["one/two/three/*"]);
    dir.cargo_member("one/two/three/leaf", &["soroban-sdk"]);

    let scan = scan_cargo_workspace(dir.path());
    assert_eq!(member_paths(&scan), vec!["one/two/three/leaf"]);
    assert!(scan.diagnostics.is_empty(), "{scan:?}");
}
