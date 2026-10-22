//! Loading a directory of fixture files, with their CF-04 verification
//! evidence.
//!
//! A fixture's verification record can be embedded as a `[verification]`
//! table (parsed strictly by the manifest; an invalid table is an invalid
//! fixture) or carried in an evidence *sidecar* file: a JSON document
//! named `<name>.evidence.json`, either beside the fixture or anywhere
//! under the loaded directory (conventionally in an `evidence/`
//! directory), keyed by the fixture's id or by the fixture file's own
//! stem. Sidecars are optional data, so a sidecar that is malformed,
//! oversized, or breaks a CF-04 rule is *reported as a warning and
//! ignored*: the fixture loads as unverified instead of failing the
//! whole directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use canary_core::CanaryError;

use crate::manifest::{parse_fixture_file, LoadedFixture};
use crate::verification::VerificationRecord;

/// Errors produced while loading and validating a fixture directory.
///
/// A `FixtureError` means the fixture input itself is unusable — an
/// unreadable file or directory, malformed TOML, a duplicate fixture id, or
/// a reference to a companion file that does not exist — as opposed to a
/// compatibility failure in the code under test. Every variant carries the
/// offending [`PathBuf`], so callers can point users at the exact file that
/// needs fixing.
///
/// It converts into [`CanaryError::Fixture`], which the CLI maps to the
/// dedicated invalid-fixture exit code rather than a generic internal error.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
///
/// use canary_fixtures::FixtureError;
///
/// let error = FixtureError::DuplicateId {
///     id: "p28-xdr-example".to_string(),
///     first: PathBuf::from("fixtures/a.toml"),
///     second: PathBuf::from("fixtures/b.toml"),
/// };
///
/// assert!(error.to_string().contains("duplicate fixture id"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("failed to read fixture file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse fixture file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    /// A fixture file is larger than [`MAX_FIXTURE_BYTES`]. Fixture bodies
    /// are small TOML documents; an oversized one is a mistake or an attempt
    /// to exhaust memory, and it is refused before it is read.
    #[error("fixture file {path} is {size} bytes, over the {limit}-byte limit")]
    FileTooLarge {
        path: PathBuf,
        size: u64,
        limit: u64,
    },

    #[error("failed to read fixture directory {path}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid [verification] table in {source_path}: {reason}")]
    InvalidVerification {
        source_path: PathBuf,
        reason: String,
    },

    #[error("duplicate fixture id {id:?}: defined in both {first} and {second}")]
    DuplicateId {
        id: String,
        first: PathBuf,
        second: PathBuf,
    },

    /// An entry inside the fixture directory is a symbolic link (or a
    /// Windows junction). Links are never followed: a link could point
    /// outside the fixture directory or form a cycle.
    #[error("unsafe entry in fixture directory: {path} is a symbolic link; fixture directories must not contain links")]
    SymbolicLink { path: PathBuf },

    /// A fixture's `input_file` or `expected_file` is not a plain relative
    /// path that stays inside the fixture's own directory tree.
    #[error("fixture file {source_path} has an unsafe {field} {reference:?}: {reason}")]
    UnsafeReference {
        source_path: PathBuf,
        field: &'static str,
        reference: String,
        reason: &'static str,
    },

    #[error(
        "fixture {id:?} ({source_path}) references a {kind} file that does not exist: {referenced}"
    )]
    MissingReferencedFile {
        id: String,
        source_path: PathBuf,
        kind: &'static str,
        referenced: PathBuf,
    },
}

impl From<FixtureError> for CanaryError {
    fn from(error: FixtureError) -> Self {
        CanaryError::Fixture(error.to_string())
    }
}

/// Name of the version-control directory that the walk never enters.
const VCS_DIR: &str = ".git";

/// Suffix of a verification evidence sidecar file: a fixture
/// `p28-rpc-001.toml` is backed by `p28-rpc-001.evidence.json`.
const EVIDENCE_SUFFIX: &str = ".evidence.json";

/// Largest fixture file the loader accepts, in bytes (1 MiB).
///
/// A fixture is a short TOML document; anything this large is not one. The
/// cap is checked from the directory entry's metadata, so an oversized file
/// is rejected before it is read into memory.
pub const MAX_FIXTURE_BYTES: u64 = 1024 * 1024;

/// One verification evidence sidecar the loader could not use.
///
/// Evidence is optional data, so a warning never fails a load: the
/// fixture it would have verified stays in the unverified state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationWarning {
    /// The evidence file the loader ignored.
    pub path: PathBuf,
    /// Why it was ignored.
    pub reason: String,
}

impl std::fmt::Display for VerificationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ignored verification evidence file {}: {}",
            self.path.display(),
            self.reason
        )
    }
}

/// The fixtures of a loaded directory plus the evidence sidecars that
/// could not be used, returned together by
/// [`load_directory_with_evidence`].
#[derive(Debug)]
pub struct LoadOutcome {
    /// The loaded fixtures, in deterministic (sorted-by-path) order,
    /// each carrying its verification record when one could be read.
    pub fixtures: Vec<LoadedFixture>,
    /// Every evidence sidecar that was ignored, in load order.
    pub warnings: Vec<VerificationWarning>,
}

/// Recursively loads every `*.toml` fixture file under `dir`, attaching
/// the verification evidence of each fixture.
///
/// Returns fixtures in a deterministic (sorted-by-path) order so that
/// downstream planning and reporting stay reproducible. Evidence
/// sidecars that cannot be used are printed to stderr as warnings and
/// do not fail the load; use [`load_directory_with_evidence`] to inspect
/// them programmatically.
///
/// Traversal rules, identical on every platform:
///
/// - `dir` itself may be a symbolic link (the caller chose it), but nothing
///   below it may be: any symbolic link or junction, to a file or a
///   directory, is rejected with [`FixtureError::SymbolicLink`]. Links are
///   never followed, so a link cannot leave the fixture tree or form a loop.
/// - A directory named `.git` is not entered. Pointing `--fixtures-dir` at
///   a repository checkout is documented and supported; its history is not
///   fixture data.
/// - Entries that are neither regular files nor directories are ignored.
/// - `*.evidence.json` files are verification evidence sidecars; every
///   other non-TOML file (including a captured request/response named by
///   a record's `evidenceRef`) is data and is not read.
pub fn load_directory(dir: &Path) -> Result<Vec<LoadedFixture>, FixtureError> {
    let outcome = load_directory_with_evidence(dir)?;
    for warning in &outcome.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(outcome.fixtures)
}

/// [`load_directory`] plus the evidence sidecars that could not be used.
///
/// An embedded `[verification]` table always wins over a sidecar; when
/// both exist the sidecar is ignored with a warning. A sidecar is looked
/// up by the fixture's id first, then by the fixture file's stem, and is
/// attached only when its own `fixtureId` matches and it satisfies the
/// CF-04 section 3 rules — otherwise the fixture stays unverified and
/// the reason is reported in [`LoadOutcome::warnings`].
pub fn load_directory_with_evidence(dir: &Path) -> Result<LoadOutcome, FixtureError> {
    let mut paths = Vec::new();
    let mut evidence_paths = Vec::new();
    let mut warnings = Vec::new();
    collect_files(dir, &mut paths, &mut evidence_paths, &mut warnings)?;
    paths.sort();
    evidence_paths.sort();

    let mut fixtures: Vec<LoadedFixture> = paths
        .iter()
        .map(|path| parse_fixture_file(path))
        .collect::<Result<_, _>>()?;

    let sidecars = index_evidence(evidence_paths, &mut warnings);
    attach_evidence(&mut fixtures, sidecars, &mut warnings);

    Ok(LoadOutcome { fixtures, warnings })
}

/// Indexes evidence sidecar files by their stem (the file name without
/// the `.evidence.json` suffix). When two directories hold a sidecar
/// with the same stem, the first in sorted order is indexed and the
/// others are reported.
fn index_evidence(
    paths: Vec<PathBuf>,
    warnings: &mut Vec<VerificationWarning>,
) -> BTreeMap<String, PathBuf> {
    let mut index: BTreeMap<String, PathBuf> = BTreeMap::new();
    for path in paths {
        let Some(stem) = evidence_stem(&path) else {
            continue;
        };
        match index.get(&stem) {
            Some(first) => warnings.push(VerificationWarning {
                path,
                reason: format!(
                    "duplicate evidence for {stem:?}: {} already provides it",
                    first.display()
                ),
            }),
            None => {
                index.insert(stem, path);
            }
        }
    }
    index
}

/// The lookup key of an evidence sidecar: its file name without the
/// [`.evidence.json`](EVIDENCE_SUFFIX) suffix, or `None` when the file
/// name is not valid UTF-8 (never the case for a name this walk
/// accepted, but not worth panicking over).
fn evidence_stem(path: &Path) -> Option<String> {
    path.file_name()?
        .to_str()?
        .strip_suffix(EVIDENCE_SUFFIX)
        .map(str::to_owned)
}

/// Attaches each available sidecar to its fixture and reports the rest.
///
/// Each sidecar is claimed at most once: the first fixture whose id or
/// file stem matches it, even if reading it later fails, so one broken
/// sidecar is reported once and never leaks onto another fixture.
fn attach_evidence(
    fixtures: &mut [LoadedFixture],
    mut sidecars: BTreeMap<String, PathBuf>,
    warnings: &mut Vec<VerificationWarning>,
) {
    for fixture in fixtures.iter_mut() {
        let stem = fixture
            .source_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_owned);
        let candidates: Vec<String> = std::iter::once(fixture.metadata.id.clone())
            .chain(stem)
            .collect();

        if fixture.verification.is_some() {
            // The embedded table is authoritative; a sidecar beside it is
            // a data conflict the maintainer should know about.
            if let Some((key, path)) = candidates.iter().find_map(|key| sidecars.remove_entry(key))
            {
                warnings.push(VerificationWarning {
                    path,
                    reason: format!(
                        "fixture {:?} already has an embedded [verification] table; the embedded table wins over the {key:?} sidecar",
                        fixture.metadata.id
                    ),
                });
            }
            continue;
        }

        let Some((_, path)) = candidates.iter().find_map(|key| sidecars.remove_entry(key)) else {
            continue;
        };
        match read_sidecar_record(&path, &fixture.metadata.id) {
            Ok(record) => fixture.verification = Some(record),
            Err(reason) => warnings.push(VerificationWarning { path, reason }),
        }
    }

    // Sidecars nothing claimed: evidence for a fixture that is not in
    // this directory, a misnamed record, or a second sidecar for a
    // fixture that already had one.
    for (_stem, path) in sidecars {
        warnings.push(VerificationWarning {
            path,
            reason: "no fixture in this directory claims this evidence file; it must be named after a fixture's id or file stem"
                .to_string(),
        });
    }
}

/// Reads one sidecar file and validates it against the CF-04 section 3
/// rules, returning the reason it cannot be used otherwise.
fn read_sidecar_record(path: &Path, fixture_id: &str) -> Result<VerificationRecord, String> {
    // The walk already refuses symlinks and oversized files; re-checking
    // keeps this function correct when called on a path that did not
    // come from the walk.
    let meta = std::fs::symlink_metadata(path).map_err(|err| format!("cannot read: {err}"))?;
    if !meta.file_type().is_file() {
        return Err("not a regular file".to_string());
    }
    if meta.len() > MAX_FIXTURE_BYTES {
        return Err(format!(
            "the file is {} bytes, over the {}-byte limit",
            meta.len(),
            MAX_FIXTURE_BYTES
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|err| format!("cannot read: {err}"))?;
    let record: VerificationRecord =
        serde_json::from_str(&text).map_err(|err| format!("invalid verification record: {err}"))?;
    record.check(fixture_id).map_err(str::to_owned)?;
    Ok(record)
}

fn collect_files(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    evidence: &mut Vec<PathBuf>,
    warnings: &mut Vec<VerificationWarning>,
) -> Result<(), FixtureError> {
    let entries = std::fs::read_dir(dir).map_err(|source| FixtureError::ReadDir {
        path: dir.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| FixtureError::ReadDir {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        // `DirEntry::file_type` does not follow links, unlike `Path::is_dir`.
        let file_type = entry.file_type().map_err(|source| FixtureError::ReadDir {
            path: path.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            return Err(FixtureError::SymbolicLink { path });
        }
        if file_type.is_dir() {
            if entry.file_name() == VCS_DIR {
                continue;
            }
            collect_files(&path, out, evidence, warnings)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "toml") {
            let size = entry
                .metadata()
                .map_err(|source| FixtureError::Read {
                    path: path.clone(),
                    source,
                })?
                .len();
            if size > MAX_FIXTURE_BYTES {
                return Err(FixtureError::FileTooLarge {
                    path,
                    size,
                    limit: MAX_FIXTURE_BYTES,
                });
            }
            out.push(path);
        } else if file_type.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(EVIDENCE_SUFFIX))
        {
            // Evidence is optional data: an oversized sidecar is skipped
            // with a warning, never a load failure.
            let size = entry
                .metadata()
                .map_err(|source| FixtureError::Read {
                    path: path.clone(),
                    source,
                })?
                .len();
            if size > MAX_FIXTURE_BYTES {
                warnings.push(VerificationWarning {
                    path,
                    reason: format!(
                        "the file is {size} bytes, over the {MAX_FIXTURE_BYTES}-byte limit"
                    ),
                });
            } else {
                evidence.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn fixture_toml(id: &str, surface: &str) -> String {
        format!(
            r#"
            id = "{id}"
            protocol = 28
            surface = "{surface}"
            category = "test"
            description = "test fixture"
            "#
        )
    }

    #[test]
    fn loads_fixtures_recursively_in_deterministic_order() {
        let dir = crate::test_support::temp_dir("loader-recursive");
        write(
            &dir.path.join("xdr/p28-xdr-001.toml"),
            &fixture_toml("p28-xdr-001", "xdr"),
        );
        write(
            &dir.path.join("rpc/p28-rpc-001.toml"),
            &fixture_toml("p28-rpc-001", "rpc"),
        );

        let fixtures = load_directory(&dir.path).expect("loads");
        assert_eq!(fixtures.len(), 2);
        let ids: Vec<_> = fixtures.iter().map(|f| f.metadata.id.clone()).collect();
        assert_eq!(ids, vec!["p28-rpc-001", "p28-xdr-001"]);
    }

    #[test]
    fn ignores_non_toml_files() {
        let dir = crate::test_support::temp_dir("loader-ignore");
        write(&dir.path.join("README.md"), "not a fixture");
        write(
            &dir.path.join("p28-xdr-001.toml"),
            &fixture_toml("p28-xdr-001", "xdr"),
        );

        let fixtures = load_directory(&dir.path).expect("loads");
        assert_eq!(fixtures.len(), 1);
    }

    #[test]
    fn propagates_parse_errors_with_the_offending_path() {
        let dir = crate::test_support::temp_dir("loader-bad-parse");
        write(&dir.path.join("broken.toml"), "not valid [[[ toml");

        let err = load_directory(&dir.path).unwrap_err();
        assert!(matches!(err, FixtureError::Parse { .. }));
    }

    fn fixture_with_reference(id: &str, field: &str, value: &str) -> String {
        format!("{}{field} = {value}\n", fixture_toml(id, "xdr"))
    }

    #[test]
    fn does_not_enter_a_git_directory() {
        let dir = crate::test_support::temp_dir("loader-dot-git");
        write(
            &dir.path.join("p28-xdr-001.toml"),
            &fixture_toml("p28-xdr-001", "xdr"),
        );
        // Not a fixture; would fail to parse if the walk entered it.
        write(&dir.path.join(".git/hooks/sample.toml"), "not [[[ toml");

        let fixtures = load_directory(&dir.path).expect("loads");
        assert_eq!(fixtures.len(), 1);
    }

    #[test]
    fn still_enters_other_hidden_directories() {
        let dir = crate::test_support::temp_dir("loader-hidden");
        write(
            &dir.path.join(".hidden/p28-xdr-001.toml"),
            &fixture_toml("p28-xdr-001", "xdr"),
        );

        assert_eq!(load_directory(&dir.path).expect("loads").len(), 1);
    }

    #[test]
    fn rejects_unsafe_payload_references() {
        let cases = [
            ("\"../outside.bin\"", "escapes"),
            ("\"a/../../outside.bin\"", "escapes"),
            ("\"/etc/passwd\"", "absolute"),
            ("\"C:/Windows/win.ini\"", "drive"),
            ("\"C:evil.bin\"", "drive"),
            ("\"dir\\\\file.bin\"", "backslash"),
            ("\"\"", "empty"),
            ("\"a//b.bin\"", "empty segment"),
            ("\"./a.bin\"", "'.' segment"),
            ("\"a/\"", "empty segment"),
        ];
        for (value, expect) in cases {
            for field in ["input_file", "expected_file"] {
                let dir = crate::test_support::temp_dir("loader-unsafe-ref");
                write(
                    &dir.path.join("p28-xdr-001.toml"),
                    &fixture_with_reference("p28-xdr-001", field, value),
                );
                let err = load_directory(&dir.path).unwrap_err();
                match &err {
                    FixtureError::UnsafeReference { field: f, .. } => assert_eq!(*f, field),
                    other => panic!("{value} in {field}: expected UnsafeReference, got {other}"),
                }
                assert!(
                    err.to_string().contains(expect),
                    "{value}: message {err} should mention {expect}"
                );
            }
        }
    }

    #[test]
    fn accepts_a_nested_relative_payload_reference() {
        let dir = crate::test_support::temp_dir("loader-good-ref");
        write(
            &dir.path.join("p28-xdr-001.toml"),
            &fixture_with_reference("p28-xdr-001", "input_file", "\"data/in.bin\""),
        );
        write(&dir.path.join("data/in.bin"), "payload");

        let fixtures = load_directory(&dir.path).expect("loads");
        assert_eq!(
            fixtures[0].input_file.as_deref(),
            Some(dir.path.join("data").join("in.bin").as_path())
        );
        crate::validate(&fixtures).expect("payload exists");
    }

    #[cfg(unix)]
    mod symlinks {
        use super::*;
        use std::os::unix::fs::symlink;

        #[test]
        fn rejects_a_symlinked_fixture_file() {
            let dir = crate::test_support::temp_dir("loader-link-file");
            let outside = crate::test_support::temp_dir("loader-link-file-target");
            write(
                &outside.path.join("real.toml"),
                &fixture_toml("p28-xdr-001", "xdr"),
            );
            symlink(outside.path.join("real.toml"), dir.path.join("link.toml")).unwrap();

            let err = load_directory(&dir.path).unwrap_err();
            assert!(matches!(err, FixtureError::SymbolicLink { .. }), "{err}");
        }

        #[test]
        fn rejects_a_symlinked_directory_that_points_outside() {
            let dir = crate::test_support::temp_dir("loader-link-dir");
            let outside = crate::test_support::temp_dir("loader-link-dir-target");
            write(
                &outside.path.join("p28-xdr-001.toml"),
                &fixture_toml("p28-xdr-001", "xdr"),
            );
            symlink(&outside.path, dir.path.join("escape")).unwrap();

            let err = load_directory(&dir.path).unwrap_err();
            assert!(matches!(err, FixtureError::SymbolicLink { .. }), "{err}");
        }

        #[test]
        fn rejects_a_directory_loop_instead_of_recursing() {
            let dir = crate::test_support::temp_dir("loader-link-loop");
            write(&dir.path.join("sub/keep.txt"), "x");
            symlink(&dir.path, dir.path.join("sub/loop")).unwrap();

            let err = load_directory(&dir.path).unwrap_err();
            assert!(matches!(err, FixtureError::SymbolicLink { .. }), "{err}");
        }

        #[test]
        fn rejects_a_non_fixture_symlink_too() {
            // The rule is "no links anywhere in the tree", not only on
            // files that look like fixtures.
            let dir = crate::test_support::temp_dir("loader-link-readme");
            write(&dir.path.join("target.txt"), "x");
            symlink(dir.path.join("target.txt"), dir.path.join("README")).unwrap();

            assert!(matches!(
                load_directory(&dir.path).unwrap_err(),
                FixtureError::SymbolicLink { .. }
            ));
        }

        #[test]
        fn accepts_a_fixtures_dir_that_is_itself_a_symlink() {
            let real = crate::test_support::temp_dir("loader-root-real");
            let holder = crate::test_support::temp_dir("loader-root-holder");
            write(
                &real.path.join("p28-xdr-001.toml"),
                &fixture_toml("p28-xdr-001", "xdr"),
            );
            let link = holder.path.join("fixtures");
            symlink(&real.path, &link).unwrap();

            assert_eq!(load_directory(&link).expect("loads").len(), 1);
        }

        #[test]
        fn validate_rejects_a_symlinked_payload() {
            let dir = crate::test_support::temp_dir("loader-link-payload");
            write(&dir.path.join("real.bin"), "payload");
            // Load first (the link does not exist yet), then swap the
            // payload for a link, as a time-of-check change would.
            write(
                &dir.path.join("p28-xdr-001.toml"),
                &fixture_with_reference("p28-xdr-001", "input_file", "\"in.bin\""),
            );
            write(&dir.path.join("in.bin"), "payload");
            let fixtures = load_directory(&dir.path).expect("loads");
            std::fs::remove_file(dir.path.join("in.bin")).unwrap();
            symlink(dir.path.join("real.bin"), dir.path.join("in.bin")).unwrap();

            assert!(matches!(
                crate::validate(&fixtures).unwrap_err(),
                FixtureError::SymbolicLink { .. }
            ));
        }
    }

    /// Windows has two kinds of link the loader must refuse: symbolic links
    /// (which need the "create symbolic links" privilege, held by an elevated
    /// shell and by GitHub's Windows runners) and junctions (directory links
    /// that need no privilege). Rust reports both through
    /// `FileType::is_symlink`; these tests pin that on the real filesystem.
    #[cfg(windows)]
    mod windows_links {
        use super::*;
        use std::process::Command;

        /// Creates a junction `link` -> `target` with `mklink /J`.
        fn make_junction(link: &Path, target: &Path) {
            // `cmd` reads a `/` in an argument as the start of a switch
            // ("Invalid switch - loop"), so hand it backslash paths only.
            let native = |p: &Path| p.display().to_string().replace('/', "\\");
            let status = Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(native(link))
                .arg(native(target))
                .output()
                .expect("cmd is available on Windows");
            assert!(
                status.status.success(),
                "mklink /J failed: {}{}",
                String::from_utf8_lossy(&status.stdout),
                String::from_utf8_lossy(&status.stderr)
            );
        }

        /// Runs `create`; if the process lacks the symlink privilege
        /// (ERROR_PRIVILEGE_NOT_HELD, 1314) the test is skipped with a note
        /// instead of failing for a reason unrelated to the loader.
        fn symlink_or_skip(create: impl FnOnce() -> std::io::Result<()>) -> bool {
            match create() {
                Ok(()) => true,
                Err(e) if e.raw_os_error() == Some(1314) => {
                    eprintln!("skipped: symbolic links need a privilege this process lacks");
                    false
                }
                Err(e) => panic!("could not create a symbolic link: {e}"),
            }
        }

        #[test]
        fn rejects_a_junction_to_a_directory_outside() {
            let dir = crate::test_support::temp_dir("loader-junction");
            let outside = crate::test_support::temp_dir("loader-junction-target");
            write(
                &outside.path.join("p28-xdr-001.toml"),
                &fixture_toml("p28-xdr-001", "xdr"),
            );
            make_junction(&dir.path.join("escape"), &outside.path);

            let err = load_directory(&dir.path).unwrap_err();
            assert!(matches!(err, FixtureError::SymbolicLink { .. }), "{err}");
        }

        #[test]
        fn rejects_a_junction_loop_instead_of_recursing() {
            let dir = crate::test_support::temp_dir("loader-junction-loop");
            write(&dir.path.join("sub/keep.txt"), "x");
            make_junction(&dir.path.join("sub/loop"), &dir.path);

            let err = load_directory(&dir.path).unwrap_err();
            assert!(matches!(err, FixtureError::SymbolicLink { .. }), "{err}");
        }

        #[test]
        fn rejects_a_directory_symbolic_link() {
            let dir = crate::test_support::temp_dir("loader-win-symlink-dir");
            let outside = crate::test_support::temp_dir("loader-win-symlink-dir-target");
            write(
                &outside.path.join("p28-xdr-001.toml"),
                &fixture_toml("p28-xdr-001", "xdr"),
            );
            if !symlink_or_skip(|| {
                std::os::windows::fs::symlink_dir(&outside.path, dir.path.join("escape"))
            }) {
                return;
            }
            let err = load_directory(&dir.path).unwrap_err();
            assert!(matches!(err, FixtureError::SymbolicLink { .. }), "{err}");
        }

        #[test]
        fn rejects_a_file_symbolic_link() {
            let dir = crate::test_support::temp_dir("loader-win-symlink-file");
            let outside = crate::test_support::temp_dir("loader-win-symlink-file-target");
            write(
                &outside.path.join("real.toml"),
                &fixture_toml("p28-xdr-001", "xdr"),
            );
            if !symlink_or_skip(|| {
                std::os::windows::fs::symlink_file(
                    outside.path.join("real.toml"),
                    dir.path.join("link.toml"),
                )
            }) {
                return;
            }
            let err = load_directory(&dir.path).unwrap_err();
            assert!(matches!(err, FixtureError::SymbolicLink { .. }), "{err}");
        }

        #[test]
        fn accepts_a_fixtures_dir_that_is_itself_a_junction() {
            let real = crate::test_support::temp_dir("loader-win-root-real");
            let holder = crate::test_support::temp_dir("loader-win-root-holder");
            write(
                &real.path.join("p28-xdr-001.toml"),
                &fixture_toml("p28-xdr-001", "xdr"),
            );
            let link = holder.path.join("fixtures");
            make_junction(&link, &real.path);

            assert_eq!(load_directory(&link).expect("loads").len(), 1);
        }
    }
}
