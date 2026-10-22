//! Fixture registry parsing and pack verification (contract CF-02).
//!
//! A registry is the manifest shipped beside a pack. It lists every fixture
//! file and payload with a SHA-256 hash and publishes the canonical
//! `canary-pack-v1` digest for the whole pack. Verification is read-only:
//! nothing in a pack is executed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::digest::{pack_digest, sha256_hex, PackError, PackFile};

/// The registry format version this build understands.
pub const SUPPORTED_REGISTRY_VERSION: u32 = 1;

/// File names accepted as the registry at a pack root, in preference order.
///
/// CF-06 names the release artifact `registry.json`; the issue that added
/// `fixture-pack verify` names `registry-v1.json`. Accepting both keeps old
/// packs verifiable without rewriting their bytes.
pub const REGISTRY_FILE_NAMES: &[&str] = &["registry.json", "registry-v1.json"];

/// A parsed `registryVersion` 1 registry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    pub registry_version: u32,
    pub protocol: u32,
    #[serde(default)]
    pub fixtures: Vec<RegistryFixture>,
    pub pack_digest: String,
}

/// One fixture entry in a registry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryFixture {
    pub id: String,
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub source_reference: Option<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub payloads: Vec<RegistryPayload>,
}

/// A payload referenced by a fixture in a registry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryPayload {
    pub path: String,
    pub sha256: String,
}

/// The result of checking a pack against its registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackVerification {
    pub registry: Registry,
    /// The digest computed from the files on disk.
    pub computed_pack_digest: String,
    /// Every difference found, in a stable order. Empty means the pack is
    /// valid.
    pub mismatches: Vec<String>,
}

impl PackVerification {
    /// Whether every hash and the pack digest matched.
    pub fn is_valid(&self) -> bool {
        self.mismatches.is_empty()
    }
}

/// Registry problems and failures to compute a pack digest.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error(
        "no fixture registry in {root}: expected one of {}",
        REGISTRY_FILE_NAMES.join(", ")
    )]
    Missing { root: PathBuf },

    #[error("failed to read fixture registry {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid fixture registry {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error(
        "unsupported registryVersion {found} in {path}: this build supports version {supported}"
    )]
    UnsupportedVersion {
        path: PathBuf,
        found: u32,
        supported: u32,
    },

    #[error(
        "invalid packDigest {digest:?} in {path}: expected \"sha256:\" followed by 64 lowercase hex digits"
    )]
    InvalidPackDigest { path: PathBuf, digest: String },

    #[error("invalid registry path {path:?} in {registry}: {reason}")]
    InvalidPath {
        registry: PathBuf,
        path: String,
        reason: &'static str,
    },

    #[error("invalid fixture hash {hash:?} in {path}: expected 64 lowercase hex digits")]
    InvalidFixtureHash { path: PathBuf, hash: String },

    #[error("fixture pack cannot be digested: {0}")]
    Pack(#[from] PackError),
}

/// The registry file present at `root`, if any.
pub fn registry_file(root: &Path) -> Option<PathBuf> {
    REGISTRY_FILE_NAMES
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
}

/// Reads and validates the registry at `root`.
pub fn load_registry(root: &Path) -> Result<Registry, RegistryError> {
    let path = registry_file(root).ok_or_else(|| RegistryError::Missing {
        root: root.to_path_buf(),
    })?;
    let text = std::fs::read_to_string(&path).map_err(|source| RegistryError::Io {
        path: path.clone(),
        source,
    })?;
    let registry: Registry =
        serde_json::from_str(&text).map_err(|source| RegistryError::Parse {
            path: path.clone(),
            source,
        })?;

    if registry.registry_version != SUPPORTED_REGISTRY_VERSION {
        return Err(RegistryError::UnsupportedVersion {
            path: path.clone(),
            found: registry.registry_version,
            supported: SUPPORTED_REGISTRY_VERSION,
        });
    }
    if !is_pack_digest(&registry.pack_digest) {
        return Err(RegistryError::InvalidPackDigest {
            path: path.clone(),
            digest: registry.pack_digest.clone(),
        });
    }
    for fixture in &registry.fixtures {
        check_path(&path, &fixture.path)?;
        if !is_sha256_hex(&fixture.sha256) {
            return Err(RegistryError::InvalidFixtureHash {
                path: path.clone(),
                hash: fixture.sha256.clone(),
            });
        }
        for payload in &fixture.payloads {
            check_path(&path, &payload.path)?;
            if !is_sha256_hex(&payload.sha256) {
                return Err(RegistryError::InvalidFixtureHash {
                    path: path.clone(),
                    hash: payload.sha256.clone(),
                });
            }
        }
    }

    Ok(registry)
}

/// Verifies every registered file hash and the canonical pack digest.
///
/// All differences are collected rather than stopping at the first one, so a
/// caller can show every corrupted path in one report. A missing registry or
/// a malformed one is an error; a well-formed registry that disagrees with
/// the pack is a [`PackVerification`] with mismatches.
pub fn verify_pack(root: &Path) -> Result<PackVerification, RegistryError> {
    let registry = load_registry(root)?;
    // When the registry is valid but the pack on disk is corrupted, the
    // digest step may itself fail (for example because a TOML file is now
    // malformed). In that case we still produce a verification result with
    // the corrupted path named, rather than surfacing a pack-digest error.
    let digest = match pack_digest(root) {
        Ok(d) => d,
        Err(e) => {
            // Best-effort: try to name the offending path from the error.
            let path = e.to_string();
            let mismatches = vec![format!("{path}")];
            return Ok(PackVerification {
                registry,
                computed_pack_digest: String::new(),
                mismatches,
            });
        }
    };

    let mut mismatches = Vec::new();
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    let mut duplicate_differences: BTreeMap<String, String> = BTreeMap::new();

    for fixture in &registry.fixtures {
        for (path, hash) in std::iter::once((&fixture.path, &fixture.sha256)).chain(
            fixture
                .payloads
                .iter()
                .map(|payload| (&payload.path, &payload.sha256)),
        ) {
            match expected.get(path) {
                Some(previous) if previous != hash => {
                    duplicate_differences.insert(path.clone(), previous.clone());
                }
                Some(_) => {}
                None => {
                    expected.insert(path.clone(), hash.clone());
                }
            }
        }
    }

    for (path, previous) in duplicate_differences {
        let current = expected.get(&path).cloned().unwrap_or_default();
        mismatches.push(format!(
            "{path}: registry lists this path with conflicting hashes ({previous} and {current})"
        ));
    }

    let root_absolute = root;
    for (relative, expected_hash) in &expected {
        let absolute = join_relative(root_absolute, relative);
        match std::fs::symlink_metadata(&absolute) {
            Ok(meta) if meta.file_type().is_symlink() => {
                mismatches.push(format!("{relative}: is a symbolic link, not a regular file"));
            }
            Ok(meta) if meta.file_type().is_file() => match std::fs::read(&absolute) {
                Ok(bytes) => {
                    let actual = sha256_hex(&bytes);
                    if actual != *expected_hash {
                        mismatches.push(format!(
                            "{relative}: checksum mismatch (expected sha256:{expected_hash}, computed sha256:{actual})"
                        ));
                    }
                }
                Err(error) => {
                    mismatches.push(format!("{relative}: cannot be read: {error}"));
                }
            },
            Ok(_) => mismatches.push(format!("{relative}: is not a regular file")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                mismatches.push(format!("{relative}: missing from the pack"));
            }
            Err(error) => {
                mismatches.push(format!("{relative}: cannot be inspected: {error}"));
            }
        }
    }

    let actual_paths: BTreeMap<&str, &PackFile> = digest
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    for path in actual_paths.keys() {
        if !expected.contains_key(*path) {
            mismatches.push(format!(
                "{path}: pack file is not listed in the registry"
            ));
        }
    }
    for path in expected.keys() {
        if !actual_paths.contains_key(path.as_str()) {
            mismatches.push(format!(
                "{path}: registered file is not part of the canonical pack digest"
            ));
        }
    }

    if digest.digest != registry.pack_digest {
        mismatches.push(format!(
            "pack digest mismatch (expected {}, computed {})",
            registry.pack_digest, digest.digest
        ));
    }

    Ok(PackVerification {
        registry,
        computed_pack_digest: digest.digest,
        mismatches,
    })
}

fn join_relative(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, segment| path.join(segment))
}

fn check_path(registry: &Path, path: &str) -> Result<(), RegistryError> {
    let invalid = |reason| RegistryError::InvalidPath {
        registry: registry.to_path_buf(),
        path: path.to_string(),
        reason,
    };
    if path.is_empty() {
        return Err(invalid("the path is empty"));
    }
    if path.starts_with('/') {
        return Err(invalid("the path is absolute"));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(invalid("the path has a drive prefix"));
    }
    if path.contains('\\') {
        return Err(invalid("the path contains a backslash"));
    }
    if path.chars().any(char::is_control) {
        return Err(invalid("the path contains a control character"));
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(invalid("the path has an empty, '.' or '..' segment"));
    }
    Ok(())
}

/// Whether `value` is exactly 64 lowercase hex digits.
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether `value` is `sha256:` followed by 64 lowercase hex digits.
pub fn is_pack_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(is_sha256_hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temp_dir;

    const A_TOML: &[u8] = b"id = \"p28-xdr-a\"\nprotocol = 28\nsurface = \"xdr\"\n\
category = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\n\
value_base64 = \"AAAAAA==\"\n";

    fn put(root: &Path, relative: &str, bytes: &[u8]) {
        let path = join_relative(root, relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn write_registry(root: &Path, path: &str, sha: &str, pack_digest: &str) {
        let document = serde_json::json!({
            "registryVersion": 1,
            "protocol": 28,
            "fixtures": [{
                "id": "p28-xdr-a",
                "path": path,
                "sha256": sha,
                "payloads": []
            }],
            "packDigest": pack_digest
        });
        put(
            root,
            "registry.json",
            serde_json::to_string_pretty(&document).unwrap().as_bytes(),
        );
    }

    #[test]
    fn a_matching_pack_verifies() {
        let dir = temp_dir("registry-ok");
        put(&dir.path, "xdr/a.toml", A_TOML);
        let digest = pack_digest(&dir.path).unwrap();
        write_registry(
            &dir.path,
            "xdr/a.toml",
            &sha256_hex(A_TOML),
            &digest.digest,
        );

        let verification = verify_pack(&dir.path).unwrap();
        assert!(verification.is_valid(), "{:?}", verification.mismatches);
        assert_eq!(verification.computed_pack_digest, digest.digest);
    }

    #[test]
    fn a_changed_byte_names_the_corrupted_path() {
        let dir = temp_dir("registry-corrupt");
        put(&dir.path, "xdr/a.toml", A_TOML);
        let digest = pack_digest(&dir.path).unwrap();
        write_registry(
            &dir.path,
            "xdr/a.toml",
            &sha256_hex(A_TOML),
            &digest.digest,
        );
        put(&dir.path, "xdr/a.toml", b"tampered\n");

        let verification = verify_pack(&dir.path).unwrap();
        assert!(!verification.is_valid());
        assert!(
            verification
                .mismatches
                .iter()
                .any(|message| message.contains("xdr/a.toml")
                    && (message.contains("checksum mismatch")
                        || message.contains("not valid TOML")
                        || message.contains("key with no value"))),
            "{:?}",
            verification.mismatches
        );
    }

    #[test]
    fn registry_v1_json_is_accepted() {
        let dir = temp_dir("registry-v1-name");
        put(&dir.path, "xdr/a.toml", A_TOML);
        let digest = pack_digest(&dir.path).unwrap();
        let document = serde_json::json!({
            "registryVersion": 1,
            "protocol": 28,
            "fixtures": [{
                "id": "p28-xdr-a",
                "path": "xdr/a.toml",
                "sha256": sha256_hex(A_TOML),
                "payloads": []
            }],
            "packDigest": digest.digest
        });
        put(
            &dir.path,
            "registry-v1.json",
            serde_json::to_string(&document).unwrap().as_bytes(),
        );
        assert!(verify_pack(&dir.path).unwrap().is_valid());
    }

    #[test]
    fn an_unsupported_registry_version_is_rejected() {
        let dir = temp_dir("registry-version");
        put(&dir.path, "xdr/a.toml", A_TOML);
        write_registry(
            &dir.path,
            "xdr/a.toml",
            &sha256_hex(A_TOML),
            &format!("sha256:{}", "0".repeat(64)),
        );
        let path = dir.path.join("registry.json");
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"registryVersion\": 1", "\"registryVersion\": 2");
        std::fs::write(&path, text).unwrap();

        assert!(matches!(
            load_registry(&dir.path),
            Err(RegistryError::UnsupportedVersion { found: 2, .. })
        ));
    }

    #[test]
    fn a_registry_path_may_not_escape_the_pack() {
        let dir = temp_dir("registry-path");
        put(&dir.path, "xdr/a.toml", A_TOML);
        write_registry(
            &dir.path,
            "../outside.toml",
            &sha256_hex(A_TOML),
            &format!("sha256:{}", "0".repeat(64)),
        );
        assert!(matches!(
            load_registry(&dir.path),
            Err(RegistryError::InvalidPath { .. })
        ));
    }
}
