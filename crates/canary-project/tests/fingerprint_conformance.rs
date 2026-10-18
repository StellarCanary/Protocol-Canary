//! CA-04 conformance: the reference vectors plus the sensitivity and
//! insensitivity requirements of `docs/contracts/ca-04-project-fingerprint.md`
//! section 9, as required by issue #486 (unit CA-04-B).
//!
//! The reference vectors are small committed directory trees under
//! `tests/data/fingerprint/`, each paired with its expected digest in
//! `tests/data/fingerprint/expected-digests.txt`. Any reimplementation
//! of CA-04 can check itself against the same file.

use std::path::{Path, PathBuf};

use canary_core::ProjectType;
use canary_project::{compute_fingerprint, detect};

/// The committed CA-04 conformance vectors.
const VECTORS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/fingerprint");

/// A disposable directory tree, removed on drop.
///
/// Same uniqueness scheme as the crate's unit-test helper: process id,
/// nanosecond timestamp and a per-process counter, because the harness
/// runs tests in parallel threads whose clock reads can collide.
struct Tree {
    path: PathBuf,
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn temp_tree(prefix: &str) -> Tree {
    use std::sync::atomic::Ordering;

    let mut path = std::env::temp_dir();
    let unique = format!(
        "{prefix}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    path.push(unique);
    std::fs::create_dir_all(&path).unwrap();
    Tree { path }
}

impl Tree {
    fn write(&self, relative: &str, contents: impl AsRef<[u8]>) {
        let full = self.path.join(relative);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Fingerprints a tree exactly the way a run would: auto-detection
/// decides the type and capabilities, and no `[project].type` override
/// is set.
fn fingerprint_of(dir: &Path) -> String {
    let context = detect(dir);
    compute_fingerprint(dir, context.project_type, &context.capabilities, None)
        .digest()
        .to_string()
}

/// The plain Cargo manifest the sensitivity tests mutate.
const CARGO_MANIFEST: &str = r#"[package]
name = "fp-vector"
version = "0.1.0"
edition = "2021"
"#;

fn cargo_manifest_with_dependency(version: &str) -> String {
    format!("{CARGO_MANIFEST}\n[dependencies]\nsoroban-sdk = \"{version}\"\n")
}

// ---------------------------------------------------------------------------
// Reference vectors
// ---------------------------------------------------------------------------

#[test]
fn reference_vectors_match_the_committed_ca04_digests() {
    let listing = std::fs::read_to_string(Path::new(VECTORS_DIR).join("expected-digests.txt"))
        .expect("expected-digests.txt");
    let mut checked = 0;
    for line in listing.lines() {
        let (name, digest) = line.split_once(' ').expect("`<name> <digest>` lines");
        let dir = Path::new(VECTORS_DIR).join(name);
        assert!(dir.is_dir(), "vector directory {name} is missing");
        let context = detect(&dir);
        let fingerprint =
            compute_fingerprint(&dir, context.project_type, &context.capabilities, None);
        assert_eq!(fingerprint.digest(), digest, "vector {name}");
        checked += 1;
    }
    assert!(checked >= 5, "expected at least five reference vectors");
}

#[test]
fn the_reference_vector_trees_are_what_the_vectors_describe() {
    // Guard against a vector silently losing the signal it exists to
    // cover: the digest check above would still pass if the tree were
    // emptied.
    let soroban = detect(&Path::new(VECTORS_DIR).join("soroban-contract"));
    assert!(soroban.has_capability(&canary_core::Capability::SorobanContract));
    assert!(soroban.has_capability(&canary_core::Capability::WasmArtifact));
    assert_eq!(soroban.project_type, ProjectType::Soroban);

    let js = detect(&Path::new(VECTORS_DIR).join("js-rpc-consumer"));
    assert!(js.has_capability(&canary_core::Capability::RpcClient));

    let stellar_toml = detect(&Path::new(VECTORS_DIR).join("stellar-toml-generic"));
    assert_eq!(stellar_toml.project_type, ProjectType::GenericStellar);

    let minimal = detect(&Path::new(VECTORS_DIR).join("minimal-cargo"));
    assert_eq!(minimal.project_type, ProjectType::Unknown);
}

// ---------------------------------------------------------------------------
// Shape, determinism, and the documented invariances
// ---------------------------------------------------------------------------

#[test]
fn the_digest_matches_the_documented_shape() {
    let tree = temp_tree("fp-shape");
    tree.write("Cargo.toml", CARGO_MANIFEST);
    let digest = fingerprint_of(&tree.path);
    assert!(digest.starts_with("sha256:"), "{digest}");
    let hex = &digest["sha256:".len()..];
    assert_eq!(hex.len(), 64, "{digest}");
    assert!(
        hex.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "{digest}"
    );
}

#[test]
fn the_same_tree_in_two_directories_gives_one_digest() {
    let first = temp_tree("fp-path-a");
    let second = temp_tree("fp-path-b");
    for tree in [&first, &second] {
        tree.write("Cargo.toml", cargo_manifest_with_dependency("22"));
        tree.write(
            "src/lib.rs",
            "pub fn add(a: i64, b: i64) -> i64 { a + b }\n",
        );
    }
    assert_eq!(fingerprint_of(&first.path), fingerprint_of(&second.path));
}

#[test]
fn workspace_members_at_different_depths_fingerprint_independently() {
    // Two identical members of one repository have equal fingerprints,
    // by design: the digest says nothing about which directory it came
    // from (CA-04 section 4, "Monorepos").
    let repo = temp_tree("fp-monorepo");
    for member in ["members/a", "members/deeper/b"] {
        repo.write(&format!("{member}/Cargo.toml"), CARGO_MANIFEST);
    }
    let a = fingerprint_of(&repo.path.join("members/a"));
    let b = fingerprint_of(&repo.path.join("members/deeper/b"));
    assert_eq!(a, b);
}

#[test]
fn file_creation_order_does_not_change_the_digest() {
    let first = temp_tree("fp-order-a");
    first.write("Cargo.toml", CARGO_MANIFEST);
    first.write("stellar.toml", "network_passphrase = \"x\"\n");
    first.write("contract.wasm", b"\0asm\x01\x00\x00\x00");

    let second = temp_tree("fp-order-b");
    second.write("contract.wasm", b"\0asm\x01\x00\x00\x00");
    second.write("stellar.toml", "network_passphrase = \"x\"\n");
    second.write("Cargo.toml", CARGO_MANIFEST);

    assert_eq!(fingerprint_of(&first.path), fingerprint_of(&second.path));
}

#[test]
fn editing_source_comments_and_formatting_does_not_change_the_digest() {
    let tree = temp_tree("fp-cosmetic");
    tree.write("Cargo.toml", CARGO_MANIFEST);
    tree.write(
        "src/lib.rs",
        "pub fn add(a: i64, b: i64) -> i64 { a + b }\n",
    );
    let before = fingerprint_of(&tree.path);

    tree.write(
        "src/lib.rs",
        "//! Rewritten with comments and different formatting.\n\n\npub fn add(\n    a: i64,\n    b: i64,\n) -> i64 {\n    a + b // unchanged behaviour\n}\n",
    );
    assert_eq!(before, fingerprint_of(&tree.path));
}

#[test]
fn readme_lockfile_and_git_metadata_do_not_change_the_digest() {
    let tree = temp_tree("fp-ignored-files");
    tree.write("Cargo.toml", CARGO_MANIFEST);
    let before = fingerprint_of(&tree.path);

    tree.write("README.md", "# A new README\n");
    tree.write("Cargo.lock", "# lockfile bytes the detector never reads\n");
    tree.write(".git/config", "[core]\n");
    assert_eq!(before, fingerprint_of(&tree.path));
}

// ---------------------------------------------------------------------------
// The documented sensitivities (issue #486's negative tests)
// ---------------------------------------------------------------------------

#[test]
fn bumping_a_dependency_version_changes_the_digest() {
    let tree = temp_tree("fp-dependency-bump");
    tree.write("Cargo.toml", cargo_manifest_with_dependency("22"));
    let before = fingerprint_of(&tree.path);

    tree.write("Cargo.toml", cargo_manifest_with_dependency("23"));
    assert_ne!(before, fingerprint_of(&tree.path));
}

#[test]
fn adding_a_capability_changing_dependency_changes_the_digest() {
    let tree = temp_tree("fp-capability-change");
    tree.write("Cargo.toml", CARGO_MANIFEST);
    let before = fingerprint_of(&tree.path);

    // Adding soroban-sdk changes the capability list and the detected
    // type; both are covered lines, so the digest must move.
    tree.write("Cargo.toml", cargo_manifest_with_dependency("22"));
    assert_ne!(before, fingerprint_of(&tree.path));
}

#[test]
fn changing_a_wasm_artifact_by_one_byte_changes_the_digest() {
    let tree = temp_tree("fp-artifact-change");
    tree.write("Cargo.toml", cargo_manifest_with_dependency("22"));
    tree.write("contract.wasm", b"\0asm\x01\x00\x00\x00v1");
    let before = fingerprint_of(&tree.path);

    // Changing contract code reaches the fingerprint through the built
    // artifact the detector found (source files themselves are not
    // inputs).
    tree.write("contract.wasm", b"\0asm\x01\x00\x00\x00v2");
    assert_ne!(before, fingerprint_of(&tree.path));
}

#[test]
fn adding_stellar_toml_changes_the_digest() {
    let tree = temp_tree("fp-stellar-toml");
    tree.write("Cargo.toml", CARGO_MANIFEST);
    let before = fingerprint_of(&tree.path);

    tree.write("stellar.toml", "network_passphrase = \"x\"\n");
    assert_ne!(before, fingerprint_of(&tree.path));
}

#[test]
fn removing_a_manifest_changes_the_digest() {
    let tree = temp_tree("fp-manifest-removal");
    tree.write("Cargo.toml", CARGO_MANIFEST);
    tree.write("package.json", "{}\n");
    let before = fingerprint_of(&tree.path);

    std::fs::remove_file(tree.path.join("package.json")).unwrap();
    assert_ne!(before, fingerprint_of(&tree.path));
}

#[test]
fn an_explicit_type_setting_changes_the_digest() {
    let tree = temp_tree("fp-type-setting");
    tree.write("Cargo.toml", CARGO_MANIFEST);
    let context = detect(&tree.path);

    let auto = compute_fingerprint(
        &tree.path,
        context.project_type,
        &context.capabilities,
        None,
    );
    let explicit = compute_fingerprint(
        &tree.path,
        context.project_type,
        &context.capabilities,
        Some(ProjectType::Soroban),
    );
    assert_ne!(auto.digest(), explicit.digest());
}

#[test]
fn crlf_and_lf_manifests_give_different_digests() {
    // Documented behaviour (CA-04 section 4): hashing is over raw bytes
    // with no newline normalization.
    let lf = temp_tree("fp-lf");
    lf.write("Cargo.toml", CARGO_MANIFEST);
    let crlf = temp_tree("fp-crlf");
    crlf.write("Cargo.toml", CARGO_MANIFEST.replace('\n', "\r\n"));
    assert_ne!(fingerprint_of(&lf.path), fingerprint_of(&crlf.path));
}

#[test]
fn inputs_count_the_manifest_unreadable_and_artifact_lines() {
    let tree = temp_tree("fp-inputs");
    tree.write("Cargo.toml", CARGO_MANIFEST);
    let context = detect(&tree.path);
    let only_manifest = compute_fingerprint(
        &tree.path,
        context.project_type,
        &context.capabilities,
        None,
    );
    assert_eq!(only_manifest.inputs(), 1);

    tree.write("contract.wasm", b"\0asm\x01\x00\x00\x00");
    let context = detect(&tree.path);
    let with_artifact = compute_fingerprint(
        &tree.path,
        context.project_type,
        &context.capabilities,
        None,
    );
    assert_eq!(with_artifact.inputs(), 2);
}

// ---------------------------------------------------------------------------
// Unreadable inputs and non-UTF-8 paths (CA-04 section 3)
// ---------------------------------------------------------------------------

#[test]
fn an_oversized_manifest_is_recorded_and_changes_the_digest() {
    let small = temp_tree("fp-oversize-small");
    small.write("Cargo.toml", CARGO_MANIFEST);
    let empty = temp_tree("fp-oversize-empty");

    let big = temp_tree("fp-oversize-big");
    big.write(
        "Cargo.toml",
        format!("{CARGO_MANIFEST}{}", "# padding\n".repeat(120_000)),
    );

    let context = detect(&big.path);
    let fingerprint =
        compute_fingerprint(&big.path, context.project_type, &context.capabilities, None);
    // The oversized manifest is recorded (as an `unreadable` line), not
    // dropped, and it is not hashed like the small one either.
    assert_eq!(fingerprint.inputs(), 1);
    assert_ne!(fingerprint.digest(), fingerprint_of(&small.path));
    assert_ne!(fingerprint.digest(), fingerprint_of(&empty.path));
}

#[cfg(unix)]
#[test]
fn a_symlinked_manifest_is_recorded_unreadable_and_not_followed() {
    use std::os::unix::fs::symlink;

    let outside = temp_tree("fp-symlink-outside");
    outside.write("Cargo.toml", CARGO_MANIFEST);

    let linked = temp_tree("fp-symlink-linked");
    symlink(
        outside.path.join("Cargo.toml"),
        linked.path.join("Cargo.toml"),
    )
    .unwrap();

    let real = temp_tree("fp-symlink-real");
    real.write("Cargo.toml", CARGO_MANIFEST);
    let real_digest = fingerprint_of(&real.path);

    // Following the link would hash the real bytes; recording it as
    // unreadable must give a different digest.
    assert_ne!(fingerprint_of(&linked.path), real_digest);
    // It is still one input line, so it changes the digest rather than
    // disappearing.
    let context = detect(&linked.path);
    let fingerprint = compute_fingerprint(
        &linked.path,
        context.project_type,
        &context.capabilities,
        None,
    );
    assert_eq!(fingerprint.inputs(), 1);
    assert_ne!(
        fingerprint.digest(),
        fingerprint_of(&temp_tree("fp-symlink-empty").path)
    );
}

#[cfg(unix)]
#[test]
fn a_non_utf8_artifact_path_is_counted_not_hashed() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let clean = temp_tree("fp-non-utf8-clean");
    clean.write("Cargo.toml", CARGO_MANIFEST);
    clean.write("contract.wasm", b"\0asm\x01\x00\x00\x00");
    let clean_digest = fingerprint_of(&clean.path);

    let odd = temp_tree("fp-non-utf8-odd");
    odd.write("Cargo.toml", CARGO_MANIFEST);
    odd.write("contract.wasm", b"\0asm\x01\x00\x00\x00");
    let odd_name = OsString::from_vec(vec![0xFF, b'.', b'w', b'a', b's', b'm']);
    // Not every filesystem can hold such a name: APFS (macOS) rejects
    // one at creation with `EILSEQ` (illegal byte sequence). Where the
    // name cannot exist there is no behaviour to observe, so the test
    // stops here; on filesystems that accept it (CI's Linux runners) it
    // runs to the assertions below.
    if let Err(error) = std::fs::write(odd.path.join(&odd_name), b"\0asm\x01\x00\x00\x00") {
        eprintln!("skipping: this filesystem cannot store a non-UTF-8 file name: {error}");
        return;
    }

    // The non-UTF-8 artifact cannot be normalized: it is counted, which
    // still changes the digest, but it is never hashed under a lossy
    // name.
    assert_ne!(fingerprint_of(&odd.path), clean_digest);
}
