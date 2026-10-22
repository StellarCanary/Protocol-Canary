//! The canonical fixture-pack digest (`canary-pack-v1`, contract CF-02).
//!
//! A pack digest identifies the exact bytes of a fixture pack. It is *not*
//! the engine's internal per-fixture cache key ([`crate::content_digest`]);
//! it is the value a registry publishes as `packDigest` and a lockfile pins.
//!
//! The digest is `sha256:` followed by the SHA-256 of a manifest text:
//!
//! ```text
//! canary-pack-v1\n
//! <sha256 hex> <decimal byte length> <path>\n     (one line per file)
//! ```
//!
//! Files are the distinct regular `.toml` files below the pack root (the
//! `.git` directory is skipped) plus every file named by an `input_file` or
//! `expected_file` key. Lines are sorted by the UTF-8 bytes of the
//! `/`-separated path relative to the root, so the result does not depend
//! on the platform, the checkout location, directory iteration order, or
//! file modification times. Contents are hashed as raw bytes with no
//! newline or encoding normalization.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Plain SHA-256 of `bytes` as lowercase hex (no framing, unlike
/// `canary_core::sha256_hex`, which length-prefixes its parts).
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Manifest header, the first line of the hashed text.
const HEADER: &str = "canary-pack-v1\n";
/// Prefix of the returned digest.
const DIGEST_PREFIX: &str = "sha256:";
/// Name of the version-control directory that is never part of a pack.
const VCS_DIR: &str = ".git";

/// Most files a pack may contain (decision D-06).
pub const MAX_PACK_FILES: usize = 2000;
/// Most bytes all of a pack's files may total (16 MiB, decision D-06).
pub const MAX_PACK_BYTES: u64 = 16 * 1024 * 1024;
/// Most bytes one file may have (1 MiB, decision D-06).
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Deepest path, counted in `/`-separated segments (decision D-06).
pub const MAX_PATH_DEPTH: usize = 8;

/// One file that contributes to a pack digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackFile {
    /// Path relative to the pack root, `/`-separated.
    pub path: String,
    /// Lowercase hex SHA-256 of the file's bytes.
    pub sha256: String,
    /// File size in bytes.
    pub size: u64,
}

/// The result of digesting a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackDigest {
    /// `sha256:` followed by 64 lowercase hex digits.
    pub digest: String,
    /// The files that were hashed, sorted by path bytes.
    pub files: Vec<PackFile>,
}

/// A pack that cannot be digested. The input is invalid; no partial digest
/// is ever produced.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("pack contains a symbolic link: {path}; links are not allowed anywhere in a pack")]
    SymbolicLink { path: PathBuf },

    #[error("invalid pack path {path:?}: {reason}")]
    InvalidPath { path: String, reason: &'static str },

    #[error("pack paths {first:?} and {second:?} differ only by letter case")]
    CaseCollision { first: String, second: String },

    #[error("{file}: {field} {reference:?} is not usable: {reason}")]
    BadReference {
        file: String,
        field: &'static str,
        reference: String,
        reason: String,
    },

    #[error("{file} is not valid TOML: {message}")]
    InvalidToml { file: String, message: String },

    #[error("pack limit exceeded: {0}")]
    Limit(String),
}

fn io_error(path: &Path, source: std::io::Error) -> PackError {
    PackError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Computes the `canary-pack-v1` digest of the pack rooted at `root`.
///
/// # Errors
///
/// Returns a [`PackError`] when the pack is unusable: a symbolic link,
/// an unsafe or non-UTF-8 path, two paths equal after case folding, a
/// referenced payload that is missing, malformed TOML, or a limit
/// exceeded. The root itself may be a symbolic link; the caller chose it.
pub fn pack_digest(root: &Path) -> Result<PackDigest, PackError> {
    let mut tomls = Vec::new();
    walk(root, root, &mut tomls)?;

    // Distinct member paths -> absolute location, sorted by path bytes.
    let mut members: BTreeMap<String, PathBuf> = BTreeMap::new();
    for (relative, absolute) in &tomls {
        members.insert(relative.clone(), absolute.clone());
    }
    for (relative, absolute) in &tomls {
        for field in ["input_file", "expected_file"] {
            if let Some((payload_rel, payload_abs)) =
                resolve_payload(root, relative, absolute, field)?
            {
                members.insert(payload_rel, payload_abs);
            }
        }
    }

    check_path_rules(members.keys())?;
    if members.len() > MAX_PACK_FILES {
        return Err(PackError::Limit(format!(
            "{} files exceeds the maximum of {MAX_PACK_FILES}",
            members.len()
        )));
    }

    let mut files = Vec::with_capacity(members.len());
    let mut total: u64 = 0;
    for (relative, absolute) in &members {
        let meta = std::fs::symlink_metadata(absolute).map_err(|e| io_error(absolute, e))?;
        if meta.file_type().is_symlink() {
            return Err(PackError::SymbolicLink {
                path: absolute.clone(),
            });
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(PackError::Limit(format!(
                "{relative} is {} bytes, over the {MAX_FILE_BYTES}-byte per-file maximum",
                meta.len()
            )));
        }
        let bytes = std::fs::read(absolute).map_err(|e| io_error(absolute, e))?;
        let size = bytes.len() as u64;
        total += size;
        if total > MAX_PACK_BYTES {
            return Err(PackError::Limit(format!(
                "pack exceeds the {MAX_PACK_BYTES}-byte total maximum"
            )));
        }
        files.push(PackFile {
            path: relative.clone(),
            sha256: sha256_hex(&bytes),
            size,
        });
    }

    let mut manifest = String::from(HEADER);
    for file in &files {
        manifest.push_str(&format!("{} {} {}\n", file.sha256, file.size, file.path));
    }
    let digest = format!("{DIGEST_PREFIX}{}", sha256_hex(manifest.as_bytes()));
    Ok(PackDigest { digest, files })
}

/// Collects `(relative path, absolute path)` for every regular `.toml`.
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), PackError> {
    let entries = std::fs::read_dir(dir).map_err(|e| io_error(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_error(dir, e))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| io_error(&path, e))?;
        if file_type.is_symlink() {
            return Err(PackError::SymbolicLink { path });
        }
        if file_type.is_dir() {
            if entry.file_name() == VCS_DIR {
                continue;
            }
            walk(root, &path, out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "toml") {
            out.push((relative_path(root, &path)?, path));
        }
    }
    Ok(())
}

/// The `/`-separated path of `path` below `root`, which must be UTF-8.
fn relative_path(root: &Path, path: &Path) -> Result<String, PackError> {
    let stripped = path
        .strip_prefix(root)
        .map_err(|_| PackError::InvalidPath {
            path: path.to_string_lossy().into_owned(),
            reason: "is outside the pack root",
        })?;
    let mut segments = Vec::new();
    for component in stripped.components() {
        let segment = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| PackError::InvalidPath {
                path: stripped.to_string_lossy().into_owned(),
                reason: "is not valid UTF-8",
            })?;
        segments.push(segment);
    }
    Ok(segments.join("/"))
}

/// Resolves one `input_file`/`expected_file` value of a fixture file.
fn resolve_payload(
    root: &Path,
    toml_rel: &str,
    toml_abs: &Path,
    field: &'static str,
) -> Result<Option<(String, PathBuf)>, PackError> {
    // Parsed per field; packs are small and bounded by `MAX_PACK_FILES`.
    let text = std::fs::read_to_string(toml_abs).map_err(|e| io_error(toml_abs, e))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| PackError::InvalidToml {
            file: toml_rel.to_string(),
            message: e.message().to_string(),
        })?;
    let Some(value) = table.get(field) else {
        return Ok(None);
    };
    let bad = |reference: String, reason: &str| PackError::BadReference {
        file: toml_rel.to_string(),
        field,
        reference,
        reason: reason.to_string(),
    };
    let Some(reference) = value.as_str() else {
        return Err(bad(value.to_string(), "must be a string"));
    };
    if let Some(reason) = reference_problem(reference) {
        return Err(bad(reference.to_string(), reason));
    }
    let dir = toml_rel.rsplit_once('/').map_or("", |(dir, _)| dir);
    let relative = if dir.is_empty() {
        reference.to_string()
    } else {
        format!("{dir}/{reference}")
    };
    let absolute = relative
        .split('/')
        .fold(root.to_path_buf(), |p, s| p.join(s));
    match std::fs::symlink_metadata(&absolute) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(PackError::SymbolicLink { path: absolute })
        }
        Ok(meta) if meta.file_type().is_file() => Ok(Some((relative, absolute))),
        Ok(_) => Err(bad(reference.to_string(), "is not a regular file")),
        Err(_) => Err(bad(reference.to_string(), "does not exist")),
    }
}

/// Lexical rules for a payload reference: it must stay inside the pack.
fn reference_problem(reference: &str) -> Option<&'static str> {
    if reference.is_empty() {
        return Some("the path is empty");
    }
    if reference.starts_with('/') {
        return Some("the path is absolute");
    }
    let bytes = reference.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Some("the path has a drive prefix");
    }
    for segment in reference.split('/') {
        match segment {
            "" => return Some("the path has an empty segment"),
            "." => return Some("the path has a '.' segment"),
            ".." => return Some("the path escapes the pack with '..'"),
            _ => {}
        }
    }
    None
}

/// Applies the CF-02 path rules to the final member set.
fn check_path_rules<'a>(paths: impl Iterator<Item = &'a String>) -> Result<(), PackError> {
    let mut folded: BTreeMap<String, &String> = BTreeMap::new();
    for path in paths {
        let invalid = |reason: &'static str| PackError::InvalidPath {
            path: path.clone(),
            reason,
        };
        if path.is_empty() {
            return Err(invalid("is empty"));
        }
        if path.contains('\\') {
            return Err(invalid("contains a backslash"));
        }
        if path.chars().any(char::is_control) {
            return Err(invalid("contains a control character"));
        }
        if path.split('/').count() > MAX_PATH_DEPTH {
            return Err(invalid("is deeper than the 8-segment maximum"));
        }
        if path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        {
            return Err(invalid("has an empty, '.' or '..' segment"));
        }
        if let Some(first) = folded.insert(case_fold(path), path) {
            return Err(PackError::CaseCollision {
                first: first.clone(),
                second: path.clone(),
            });
        }
    }
    Ok(())
}

/// Unicode case folding, approximated as upper-then-lower per character,
/// which folds `ß` to `ss` as full case folding does. Used only to detect
/// names that would collide on a case-insensitive file system.
fn case_fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_uppercase)
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temp_dir;

    const A_TOML: &[u8] =
        b"id = \"p28-xdr-a\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"cap-0083\"\n\
description = \"golden xdr\"\nsource_reference = \"CAP-0083\"\n\
required_capabilities = [\"wasm-artifact\"]\ninput_file = \"a.bin\"\n\
type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAAAA==\"\n";
    const B_TOML: &[u8] =
        b"id = \"p28-rpc-b\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"network\"\n\
description = \"golden rpc\"\nmethod = \"get-network\"\n\n[[assert]]\n\
kind = \"field-exists\"\nfield = \"passphrase\"\n";
    const PAYLOAD: &[u8] = b"\x00\x01payload\xff\n";

    /// Computed with coreutils (`sha256sum`, `printf`), not with this code.
    const GOLDEN: &str = "sha256:8546eb66a764b16345ace35acfbd62e54229572dbe4bc41c42ad2ba65a3808b5";

    fn put(root: &Path, relative: &str, bytes: &[u8]) {
        let path = relative
            .split('/')
            .fold(root.to_path_buf(), |p, s| p.join(s));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn golden_pack(root: &Path) {
        put(root, "xdr/a.toml", A_TOML);
        put(root, "xdr/a.bin", PAYLOAD);
        put(root, "rpc/b.toml", B_TOML);
    }

    #[test]
    fn matches_the_independently_computed_golden_vector() {
        let dir = temp_dir("pack-golden");
        golden_pack(&dir.path);
        let result = pack_digest(&dir.path).unwrap();
        assert_eq!(result.digest, GOLDEN);
        let listed: Vec<(&str, u64)> = result
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.size))
            .collect();
        assert_eq!(
            listed,
            [("rpc/b.toml", 173), ("xdr/a.bin", 11), ("xdr/a.toml", 261)]
        );
    }

    #[test]
    fn creation_order_and_location_do_not_matter() {
        let one = temp_dir("pack-order-1");
        let two = temp_dir("pack-order-2");
        golden_pack(&one.path);
        put(&two.path, "rpc/b.toml", B_TOML);
        put(&two.path, "xdr/a.bin", PAYLOAD);
        put(&two.path, "xdr/a.toml", A_TOML);
        assert_eq!(
            pack_digest(&one.path).unwrap().digest,
            pack_digest(&two.path).unwrap().digest
        );
    }

    #[test]
    fn one_changed_byte_changes_the_digest() {
        let dir = temp_dir("pack-change");
        golden_pack(&dir.path);
        put(&dir.path, "xdr/a.bin", b"\x00\x01payload\xfe\n");
        assert_ne!(pack_digest(&dir.path).unwrap().digest, GOLDEN);
    }

    #[test]
    fn unreferenced_non_toml_files_and_git_are_ignored() {
        let dir = temp_dir("pack-ignore");
        golden_pack(&dir.path);
        put(&dir.path, "README.md", b"not part of the pack");
        put(&dir.path, ".git/config.toml", b"[core]\n");
        assert_eq!(pack_digest(&dir.path).unwrap().digest, GOLDEN);
    }

    #[test]
    fn a_shared_payload_is_listed_once() {
        let dir = temp_dir("pack-dedup");
        golden_pack(&dir.path);
        put(&dir.path, "xdr/c.toml", A_TOML);
        let result = pack_digest(&dir.path).unwrap();
        let bins = result
            .files
            .iter()
            .filter(|f| f.path.ends_with(".bin"))
            .count();
        assert_eq!(bins, 1);
    }

    #[test]
    fn missing_payload_is_an_error() {
        let dir = temp_dir("pack-missing");
        put(&dir.path, "xdr/a.toml", A_TOML);
        assert!(matches!(
            pack_digest(&dir.path),
            Err(PackError::BadReference { .. })
        ));
    }

    #[test]
    fn payload_may_not_escape_with_dot_dot() {
        let dir = temp_dir("pack-escape");
        put(&dir.path, "xdr/a.toml", b"input_file = \"../secret.bin\"\n");
        put(&dir.path, "secret.bin", b"x");
        assert!(matches!(
            pack_digest(&dir.path),
            Err(PackError::BadReference { .. })
        ));
    }

    #[test]
    fn names_that_differ_only_by_case_collide() {
        let dir = temp_dir("pack-case");
        put(&dir.path, "a/Fixture.toml", b"");
        put(&dir.path, "b/x.toml", b"");
        // Same directory, case-different names; skipped when the file
        // system itself is case-insensitive and merges them.
        put(&dir.path, "a/fixture.toml", b"");
        let count = std::fs::read_dir(dir.path.join("a")).unwrap().count();
        if count == 2 {
            assert!(matches!(
                pack_digest(&dir.path),
                Err(PackError::CaseCollision { .. })
            ));
        }
    }

    #[test]
    fn case_fold_handles_sharp_s() {
        assert_eq!(case_fold("Stra\u{df}e"), case_fold("STRASSE"));
    }

    #[test]
    fn path_rules_reject_unsafe_names() {
        for bad in ["a\\b.toml", "a/\u{7}.toml", "a//b.toml", "a/../b.toml"] {
            let paths = [bad.to_string()];
            assert!(check_path_rules(paths.iter()).is_err(), "{bad:?}");
        }
        let deep = ["a/b/c/d/e/f/g/h/i.toml".to_string()];
        assert!(check_path_rules(deep.iter()).is_err());
        let ok = ["a/b/c/d/e/f/g/h.toml".to_string()];
        assert!(check_path_rules(ok.iter()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_rejected() {
        let dir = temp_dir("pack-link");
        golden_pack(&dir.path);
        std::os::unix::fs::symlink(dir.path.join("xdr/a.bin"), dir.path.join("xdr/l.bin")).unwrap();
        assert!(matches!(
            pack_digest(&dir.path),
            Err(PackError::SymbolicLink { .. })
        ));
    }

    #[test]
    fn per_file_limit_is_enforced() {
        let dir = temp_dir("pack-big");
        let big = vec![b'#'; (MAX_FILE_BYTES + 1) as usize];
        put(&dir.path, "big.toml", &big);
        assert!(matches!(pack_digest(&dir.path), Err(PackError::Limit(_))));
    }
}
