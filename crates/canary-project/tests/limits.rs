//! Project scan limits tests (issue #414): depth limit 4, directory limit 2000,
//! and workspace member limit 64.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_project::{
    detect_details, scan_cargo_workspace, DiagnosticCode, MAX_DIRECTORIES_VISITED,
    MAX_WORKSPACE_DEPTH, MAX_WORKSPACE_MEMBERS,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "canary-limits-{name}-{}-{}",
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

    fn member(&self, dir: &str) {
        self.write(
            &format!("{dir}/Cargo.toml"),
            "[package]\nname = \"m\"\nversion = \"0.1.0\"\n",
        );
    }

    fn workspace(&self, members: &[&str]) {
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

#[test]
fn depth_limit_bound_records_warning() {
    let dir = Scratch::new("depth-bound");
    let too_deep = ["d"; MAX_WORKSPACE_DEPTH + 1].join("/");
    dir.member(&too_deep);
    dir.workspace(&[&too_deep]);

    let scan = scan_cargo_workspace(dir.path());
    assert!(scan.members.is_empty());
    let hit: Vec<_> = scan
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::ScanBoundHit)
        .collect();
    assert_eq!(hit.len(), 1);
    assert!(hit[0].detail.contains("4"), "{}", hit[0].detail);
}

#[test]
fn directory_limit_bound_records_warning() {
    let dir = Scratch::new("dir-bound");
    // Create MAX_DIRECTORIES_VISITED + 1 directories under dir
    for i in 0..=MAX_DIRECTORIES_VISITED {
        std::fs::create_dir_all(dir.path().join(format!("d_{i:04}"))).unwrap();
    }

    let details = detect_details(dir.path());
    let hit: Vec<_> = details
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::ScanBoundHit)
        .collect();
    assert!(
        !hit.is_empty(),
        "expected ScanBoundHit diagnostic when exceeding 2000 directories"
    );
    assert!(
        hit.iter()
            .any(|d| d.detail.contains("2000") || d.detail.contains("directories")),
        "{:?}",
        hit
    );
}

#[test]
fn workspace_members_limit_bound_records_warning() {
    let dir = Scratch::new("ws-members-bound");
    dir.workspace(&["m/*"]);
    for index in 0..MAX_WORKSPACE_MEMBERS + 5 {
        dir.member(&format!("m/c{index:03}"));
    }

    let scan = scan_cargo_workspace(dir.path());
    assert_eq!(scan.members.len(), MAX_WORKSPACE_MEMBERS);
    let hit: Vec<_> = scan
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::ScanBoundHit)
        .collect();
    assert_eq!(hit.len(), 1);
    assert!(hit[0].detail.contains("64"), "{}", hit[0].detail);
}

#[test]
fn ten_level_deep_tree_truncated_at_depth_4_with_warning() {
    let dir = Scratch::new("ten-deep");
    // Build a 10-level deep tree: d1/d2/d3/d4/d5/d6/d7/d8/d9/d10
    let path = dir.path().join("d1/d2/d3/d4/d5/d6/d7/d8/d9/d10");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("contract.wasm"), b"\0asm").unwrap();

    let details = detect_details(dir.path());
    let hit: Vec<_> = details
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::ScanBoundHit)
        .collect();
    assert!(
        !hit.is_empty(),
        "expected ScanBoundHit warning diagnostic for 10-level deep tree, got diagnostics: {:?}",
        details.diagnostics
    );
    assert!(
        hit.iter()
            .any(|d| d.detail.contains("4") || d.detail.contains("depth")),
        "{:?}",
        hit
    );
}
