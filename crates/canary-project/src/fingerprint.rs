//! The CA-04 project fingerprint: a deterministic, path-free SHA-256
//! digest over the inputs the detector actually read and the results it
//! derived from them.
//!
//! The fingerprint answers one question (see
//! `docs/contracts/ca-04-project-fingerprint.md`): "were these the same
//! inspected inputs?". It deliberately does **not** cover source code,
//! tests, documentation, lock files, the Git commit, or anything outside
//! the selected project root, so cosmetic edits and a new commit never
//! invalidate it, while a changed dependency, capability or artifact
//! always does.
//!
//! The digest input is the line-oriented text of CA-04 section 4:
//!
//! ```text
//! canary-project-fingerprint/1
//! detector <integer>
//! type-setting <auto|explicit-type>
//! type <detected-type>
//! capability <name>            one line per capability, sorted by name
//! signal stellar-toml <0|1>
//! manifest <sha256 hex> <byte length> <relative path>
//! unreadable <reason> <relative path>
//! artifact <sha256 hex or -> <byte length> <relative path>
//! non-utf8-path <count>
//! ```
//!
//! Lines of each kind are sorted by the relative path compared as UTF-8
//! bytes; the kinds appear in the order shown. The fingerprint is
//! `sha256:` followed by the lowercase hex SHA-256 of that text.

use std::path::Path;

use canary_core::{Capability, ProjectType};
use sha2::{Digest, Sha256};

/// The fingerprint construction version: the `canary-project-fingerprint/1`
/// header of the digest input. A breaking change to the construction
/// bumps this.
pub const FINGERPRINT_VERSION: u32 = 1;

/// The detector revision (CA-04 section 2). Increment this whenever
/// detection could give a different answer for the same bytes; the digest
/// changes with it, on purpose.
///
/// A unit test pins this value so a detector change cannot ship without
/// touching the fingerprint.
pub const DETECTOR_REVISION: u64 = 1;

/// The first line of the digest input, fixing the construction version.
const HEADER: &str = "canary-project-fingerprint/1";

/// Per-file size limit (D-06, CA-04 section 3): a larger manifest is
/// recorded `unreadable` with reason `too-large` instead of being hashed.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Artifacts at most this large are hashed; a larger one is recorded by
/// length only and marked `-` in the digest input.
const MAX_HASHED_ARTIFACT_BYTES: u64 = 1024 * 1024;

/// The manifests the detector reads at the project root (CA-04 unit
/// CA-04-A; later CF-07 tiers add more as they land).
const MANIFEST_NAMES: &[&str] = &["Cargo.toml", "package.json"];

/// One computed project fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFingerprint {
    digest: String,
    inputs: usize,
}

impl ProjectFingerprint {
    /// `sha256:` followed by the lowercase hex SHA-256 of the digest
    /// input (matching `^sha256:[0-9a-f]{64}$`).
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The number of `manifest`, `unreadable` and `artifact` lines that
    /// went into the digest.
    pub fn inputs(&self) -> usize {
        self.inputs
    }
}

/// Computes the CA-04 project fingerprint for the project rooted at
/// `root`.
///
/// `detected_type` and `capabilities` are what auto-detection produced
/// (e.g. from [`crate::detect`]); `type_setting` is the `[project].type`
/// configuration: `None` for `auto`, `Some(t)` for an explicit override.
/// The fingerprint records the *detected* type and the setting
/// separately, so an override is visible in the digest input without
/// hiding what detection alone produced.
///
/// This never fails: a root that cannot be listed simply contributes no
/// input lines. Inspection must not fail because of the fingerprint
/// (CA-04 section 7).
pub fn compute_fingerprint(
    root: &Path,
    detected_type: ProjectType,
    capabilities: &[Capability],
    type_setting: Option<ProjectType>,
) -> ProjectFingerprint {
    let inputs = scan_inputs(root);
    let digest_input = digest_input(
        inputs,
        detected_type,
        capabilities,
        type_setting,
        DETECTOR_REVISION,
    );
    let inputs = digest_input
        .iter()
        .filter(|line| {
            line.starts_with("manifest ")
                || line.starts_with("unreadable ")
                || line.starts_with("artifact ")
        })
        .count();
    ProjectFingerprint {
        digest: format!("sha256:{}", sha256_hex(digest_input.join("\n").as_bytes())),
        inputs,
    }
}

/// A digest-input line together with the relative path it was ranked by,
/// so each kind can be sorted by path before assembly.
#[derive(Debug)]
struct RankedLine {
    path: String,
    line: String,
}

impl RankedLine {
    fn new(path: String, line: String) -> Self {
        RankedLine { path, line }
    }
}

/// Everything the scan found, kept split by kind so the digest input can
/// emit the kinds in the contract's order.
#[derive(Debug, Default)]
struct ScannedInputs {
    has_stellar_toml: bool,
    manifests: Vec<RankedLine>,
    unreadable: Vec<RankedLine>,
    artifacts: Vec<RankedLine>,
    non_utf8_paths: u64,
}

/// Collects the CA-04 input lines for one project root.
///
/// A missing file contributes nothing (the detector never reads it); a
/// file that exists but cannot be normalized or read is recorded
/// `unreadable` with a reason code, because that changes the digest.
fn scan_inputs(root: &Path) -> ScannedInputs {
    let mut inputs = ScannedInputs {
        // The detector only tests that `stellar.toml` is a file; the
        // signal line mirrors that (CA-04 section 2).
        has_stellar_toml: root.join("stellar.toml").is_file(),
        ..ScannedInputs::default()
    };

    for name in MANIFEST_NAMES {
        let path = root.join(name);
        match std::fs::symlink_metadata(&path) {
            // Not present: nothing was read, so nothing is recorded.
            Err(_) => {}
            Ok(meta) if meta.file_type().is_symlink() => {
                // Never followed, recorded as unreadable (CA-04 section 3).
                inputs.unreadable.push(RankedLine::new(
                    (*name).to_string(),
                    format!("unreadable symlink {name}"),
                ));
            }
            Ok(meta) if meta.len() > MAX_MANIFEST_BYTES => {
                inputs.unreadable.push(RankedLine::new(
                    (*name).to_string(),
                    format!("unreadable too-large {name}"),
                ));
            }
            Ok(meta) if meta.is_file() => match std::fs::read(&path) {
                Ok(bytes) => inputs.manifests.push(RankedLine::new(
                    (*name).to_string(),
                    format!("manifest {} {} {name}", sha256_hex(&bytes), bytes.len()),
                )),
                Err(_) => inputs.unreadable.push(RankedLine::new(
                    (*name).to_string(),
                    format!("unreadable unreadable {name}"),
                )),
            },
            // A directory (or other non-file) where a manifest is
            // expected: the detector reads nothing, and there are no
            // bytes to normalize.
            Ok(_) => inputs.unreadable.push(RankedLine::new(
                (*name).to_string(),
                format!("unreadable not-a-file {name}"),
            )),
        }
    }

    for artifact in crate::detector::find_wasm_artifacts(root) {
        let Some(relative) = relative_display(root, &artifact) else {
            // A path with a non-UTF-8 component cannot be normalized; it
            // is counted, not hashed (CA-04 section 3).
            inputs.non_utf8_paths += 1;
            continue;
        };
        match std::fs::symlink_metadata(&artifact) {
            Ok(meta) if meta.file_type().is_symlink() => inputs.unreadable.push(RankedLine::new(
                relative.clone(),
                format!("unreadable symlink {relative}"),
            )),
            Ok(meta) if meta.len() <= MAX_HASHED_ARTIFACT_BYTES => match std::fs::read(&artifact) {
                Ok(bytes) => inputs.artifacts.push(RankedLine::new(
                    relative.clone(),
                    format!("artifact {} {} {relative}", sha256_hex(&bytes), bytes.len()),
                )),
                Err(_) => inputs.unreadable.push(RankedLine::new(
                    relative.clone(),
                    format!("unreadable unreadable {relative}"),
                )),
            },
            Ok(meta) => inputs.artifacts.push(RankedLine::new(
                relative.clone(),
                format!("artifact - {} {relative}", meta.len()),
            )),
            Err(_) => {}
        }
    }

    for kind in [
        &mut inputs.manifests,
        &mut inputs.unreadable,
        &mut inputs.artifacts,
    ] {
        kind.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    }
    inputs
}

/// Renders `path` relative to `root`, using `/` on every platform, or
/// `None` when any component is not valid UTF-8 (or is not a plain
/// normal component).
fn relative_display(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_str()?.to_string()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// Assembles the CA-04 section 4 digest input, one `\n`-terminated
/// style line per entry, kinds in the order the contract shows.
fn digest_input(
    inputs: ScannedInputs,
    detected_type: ProjectType,
    capabilities: &[Capability],
    type_setting: Option<ProjectType>,
    detector_revision: u64,
) -> Vec<String> {
    let mut lines = vec![
        HEADER.to_string(),
        format!("detector {detector_revision}"),
        format!(
            "type-setting {}",
            match type_setting {
                None => "auto".to_string(),
                Some(explicit) => explicit.to_string(),
            }
        ),
        format!("type {detected_type}"),
    ];

    let mut capabilities: Vec<String> = capabilities
        .iter()
        .map(|capability| format!("capability {}", capability_name(capability)))
        .collect();
    capabilities.sort();
    lines.extend(capabilities);

    lines.push(format!(
        "signal stellar-toml {}",
        if inputs.has_stellar_toml { 1 } else { 0 }
    ));

    for kind in [&inputs.manifests, &inputs.unreadable, &inputs.artifacts] {
        lines.extend(kind.iter().map(|entry| entry.line.clone()));
    }
    if inputs.non_utf8_paths > 0 {
        lines.push(format!("non-utf8-path {}", inputs.non_utf8_paths));
    }
    lines
}

/// The lowercase hex SHA-256 of `bytes` (CA-04 section 4 hashes the raw
/// digest-input text; this is deliberately *not*
/// `canary_core::sha256_hex`, which length-prefixes its parts for a
/// different construction).
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The kebab-case capability names CA-02/CA-04 use in their documents.
fn capability_name(capability: &Capability) -> &'static str {
    match capability {
        Capability::SorobanContract => "soroban-contract",
        Capability::RpcClient => "rpc-client",
        Capability::StellarSdkDependency => "stellar-sdk-dependency",
        Capability::WasmArtifact => "wasm-artifact",
        Capability::RawLedgerAccess => "raw-ledger-access",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_revision_is_pinned() {
        // A detector change that could alter detection must bump this
        // constant (and therefore every fingerprint); this test forces
        // the change to be deliberate.
        assert_eq!(DETECTOR_REVISION, 1);
        assert_eq!(FINGERPRINT_VERSION, 1);
    }

    #[test]
    fn a_detector_revision_change_changes_the_digest() {
        let dir = crate::test_support::temp_dir("fingerprint-detector-rev");
        std::fs::write(dir.path.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();

        let one = digest_input(
            scan_inputs(&dir.path),
            ProjectType::Unknown,
            &[],
            None,
            DETECTOR_REVISION,
        );
        let two = digest_input(
            scan_inputs(&dir.path),
            ProjectType::Unknown,
            &[],
            None,
            DETECTOR_REVISION + 1,
        );
        assert_ne!(
            sha256_hex(one.join("\n").as_bytes()),
            sha256_hex(two.join("\n").as_bytes())
        );
    }

    #[test]
    fn digest_matches_the_documented_shape() {
        let dir = crate::test_support::temp_dir("fingerprint-format");
        std::fs::write(dir.path.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();

        let fingerprint = compute_fingerprint(&dir.path, ProjectType::Unknown, &[], None);
        let digest = fingerprint.digest();
        assert!(digest.starts_with("sha256:"), "{digest}");
        let hex = &digest["sha256:".len()..];
        assert_eq!(hex.len(), 64);
        assert!(hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(fingerprint.inputs(), 1);
    }

    #[test]
    fn an_empty_root_fingerprints_with_no_inputs() {
        let dir = crate::test_support::temp_dir("fingerprint-empty");
        let fingerprint = compute_fingerprint(&dir.path, ProjectType::Unknown, &[], None);
        assert_eq!(fingerprint.inputs(), 0);
        assert!(fingerprint.digest().starts_with("sha256:"));
    }
}
