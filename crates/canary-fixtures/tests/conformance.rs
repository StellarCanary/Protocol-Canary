//! Cross-language fixture conformance vectors (F-022).
//!
//! `tests/data/conformance` is a vendored copy of
//! `StellarCanary/ProtocolCanary-Fixtures`'s conformance corpus. The Python
//! validator and this Rust loader must agree on every vector: all accepted
//! fixtures must load and parse through their surface, and every rejected
//! fixture must be refused with the diagnostic class the vector names.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_core::Surface;
use canary_fixtures::{load_directory, parse_fixture_file, validate, FixtureError, LoadedFixture};
use canary_rpc::RpcFixture;
use canary_soroban::SorobanFixture;
use canary_xdr::XdrFixture;
use serde::Deserialize;

const ROOT: &str = "tests/data/conformance";

#[derive(Debug, Deserialize)]
struct Manifest {
    version: u32,
    accepted: Vec<AcceptedVector>,
    rejected: Vec<RejectedVector>,
}

#[derive(Debug, Deserialize)]
struct AcceptedVector {
    id: String,
    path: String,
    surface: String,
    covers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RejectedVector {
    id: String,
    path: String,
    surface: String,
    error_code: String,
    diagnostic_contains: String,
}

#[derive(Debug, PartialEq, Eq)]
enum RejectionClass {
    Loader,
    Xdr,
    Rpc,
    Soroban,
}

#[derive(Debug)]
enum Rejection {
    Loader(FixtureError),
    Xdr(canary_xdr::XdrError),
    Rpc(canary_rpc::RpcFixtureError),
    Soroban(canary_soroban::SorobanFixtureError),
}

impl Rejection {
    fn class(&self) -> RejectionClass {
        match self {
            Rejection::Loader(_) => RejectionClass::Loader,
            Rejection::Xdr(_) => RejectionClass::Xdr,
            Rejection::Rpc(_) => RejectionClass::Rpc,
            Rejection::Soroban(_) => RejectionClass::Soroban,
        }
    }

    fn message(&self) -> String {
        match self {
            Rejection::Loader(error) => error.to_string(),
            Rejection::Xdr(error) => error.to_string(),
            Rejection::Rpc(error) => error.to_string(),
            Rejection::Soroban(error) => error.to_string(),
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(ROOT)
}

fn manifest() -> Manifest {
    let path = root().join("vectors.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn surface(name: &str) -> Surface {
    name.parse()
        .unwrap_or_else(|error| panic!("unknown surface {name:?}: {error}"))
}

fn parse_surface(loaded: &LoadedFixture) -> Result<(), Rejection> {
    match loaded.metadata.surface {
        Surface::Xdr => XdrFixture::from_loaded(loaded)
            .map(|_| ())
            .map_err(Rejection::Xdr),
        Surface::Rpc => RpcFixture::from_loaded(loaded)
            .map(|_| ())
            .map_err(Rejection::Rpc),
        Surface::Soroban => SorobanFixture::from_loaded(loaded)
            .map(|_| ())
            .map_err(Rejection::Soroban),
    }
}

/// The loader class a rejected vector is expected to fail in.
///
/// TOML syntax errors happen before a surface is known; every other vector
/// must be refused by the surface parser named in the manifest.
fn expected_class(id: &str) -> RejectionClass {
    if id == "invalid-xdr-toml-syntax" {
        RejectionClass::Loader
    } else if id.starts_with("invalid-xdr-") {
        RejectionClass::Xdr
    } else if id.starts_with("invalid-rpc-") {
        RejectionClass::Rpc
    } else if id.starts_with("invalid-soroban-") {
        RejectionClass::Soroban
    } else {
        panic!("rejected vector {id:?} has no expected rejection class")
    }
}

/// Rust-specific fragments that must appear in the rejection message for a
/// given upstream `error_code`. The upstream `diagnostic_contains` text is
/// for the Python validator and is not reproduced verbatim here.
fn expected_fragments(error_code: &str) -> &'static [&'static str] {
    match error_code {
        "invalid-toml" => &["failed to parse fixture file"],
        "missing-required-field" => &["missing required"],
        "invalid-base64" => &["is not valid base64"],
        "missing-conditional-field" => &["expected_base64", "missing required"],
        "unsupported-rpc-method" => &["unsupported rpc method"],
        "missing-rpc-assertions" => &["at least one [[assert]]"],
        "missing-rpc-assert-field" => &["missing \"field\""],
        "wrong-field-type" => &["sequence_number"],
        "unknown-soroban-expect-key" => &["unknown key(s)", "message_contain"],
        "wrong-soroban-message-type" => &["message_contains", "must be a string"],
        other => panic!("rejected vector uses an unhandled error_code {other:?}"),
    }
}

/// A scratch directory removed when dropped, so each loader test sees one
/// isolated vector and leaves nothing behind.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = format!(
            "{prefix}-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("create scratch directory");
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Loads exactly one vector file through [`load_directory`], as a caller
/// pointing the loader at a directory would.
fn load_one(source: &Path) -> Result<LoadedFixture, FixtureError> {
    let dir = TempDir::new("canary-conformance");
    let file_name = source
        .file_name()
        .unwrap_or_else(|| panic!("{} has no file name", source.display()));
    std::fs::copy(source, dir.path.join(file_name))
        .unwrap_or_else(|error| panic!("failed to copy {}: {error}", source.display()));

    let mut loaded = load_directory(&dir.path)?;
    assert_eq!(
        loaded.len(),
        1,
        "the scratch directory must contain exactly one fixture"
    );
    Ok(loaded.remove(0))
}

fn rejection_for(source: &Path) -> Rejection {
    match load_one(source) {
        Err(error) => Rejection::Loader(error),
        Ok(loaded) => match parse_surface(&loaded) {
            Ok(()) => panic!(
                "{} was accepted, but the conformance corpus marks it rejected",
                source.display()
            ),
            Err(rejection) => rejection,
        },
    }
}

fn declared_paths(manifest: &Manifest) -> BTreeSet<String> {
    manifest
        .accepted
        .iter()
        .map(|vector| vector.path.clone())
        .chain(manifest.rejected.iter().map(|vector| vector.path.clone()))
        .collect()
}

fn corpus_paths(dir: &Path) -> BTreeSet<String> {
    let prefix = if dir.ends_with("valid") {
        "valid/"
    } else {
        "invalid/"
    };
    std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .map(|path| format!("{prefix}{}", path.file_name().unwrap().to_string_lossy()))
        .collect()
}

#[test]
fn manifest_is_versioned_and_covers_every_surface() {
    let manifest = manifest();
    assert_eq!(manifest.version, 1);
    assert!(
        manifest.accepted.len() >= 10,
        "the corpus must keep at least 10 accepted vectors, found {}",
        manifest.accepted.len()
    );
    assert!(
        manifest.rejected.len() >= 10,
        "the corpus must keep at least 10 rejected vectors, found {}",
        manifest.rejected.len()
    );

    let mut ids = BTreeSet::new();
    let mut surfaces = BTreeSet::new();
    for vector in &manifest.accepted {
        assert!(!vector.id.is_empty());
        assert!(
            !vector.covers.is_empty(),
            "{} has no coverage label",
            vector.id
        );
        assert!(
            vector.path.starts_with("valid/"),
            "{} must live under valid/",
            vector.path
        );
        assert!(ids.insert(vector.id.clone()), "duplicate id {}", vector.id);
        surfaces.insert(surface(&vector.surface).to_string());
    }

    let mut rejected_surfaces = BTreeSet::new();
    for vector in &manifest.rejected {
        assert!(!vector.id.is_empty());
        assert!(
            !vector.diagnostic_contains.is_empty(),
            "{} has no upstream diagnostic",
            vector.id
        );
        assert!(
            vector.path.starts_with("invalid/"),
            "{} must live under invalid/",
            vector.path
        );
        assert!(ids.insert(vector.id.clone()), "duplicate id {}", vector.id);
        rejected_surfaces.insert(surface(&vector.surface).to_string());
        let _ = expected_fragments(&vector.error_code);
    }
    assert_eq!(
        surfaces, rejected_surfaces,
        "accepted and rejected vectors must cover the same surfaces"
    );
    assert_eq!(
        surfaces,
        BTreeSet::from(["xdr".to_string(), "rpc".to_string(), "soroban".to_string()])
    );

    let root = root();
    let declared = declared_paths(&manifest);
    let on_disk: BTreeSet<String> = corpus_paths(&root.join("valid"))
        .into_iter()
        .chain(corpus_paths(&root.join("invalid")))
        .collect();
    assert_eq!(
        declared, on_disk,
        "vectors.json and the vendored files disagree; update both together"
    );
}

#[test]
fn every_accepted_vector_loads_and_parses() {
    let manifest = manifest();
    let valid_dir = root().join("valid");
    let loaded = load_directory(&valid_dir).expect("the accepted corpus loads as a directory");
    assert_eq!(
        loaded.len(),
        manifest.accepted.len(),
        "the accepted directory and manifest must list the same number of fixtures"
    );

    let store = validate(&loaded).expect("the accepted corpus passes fixture validation");
    assert_eq!(store.len(), loaded.len());

    for vector in &manifest.accepted {
        let fixture = loaded
            .iter()
            .find(|fixture| fixture.source_path.ends_with(&vector.path))
            .unwrap_or_else(|| panic!("{} was not loaded", vector.id));
        assert_eq!(
            fixture.metadata.surface,
            surface(&vector.surface),
            "{} has the wrong surface",
            vector.id
        );
        if let Err(error) = parse_surface(fixture) {
            panic!(
                "{} is an accepted vector but the {} parser rejected it: {}",
                vector.id,
                vector.surface,
                error.message()
            );
        }

        // The file also loads on its own through `load_directory`, which is
        // how the CLI loads one fixture directory.
        let separate = load_one(&root().join(&vector.path))
            .unwrap_or_else(|error| panic!("{} failed to load: {error}", vector.id));
        assert_eq!(separate.metadata.id, fixture.metadata.id);
        parse_surface(&separate)
            .unwrap_or_else(|error| panic!("{} failed to parse: {}", vector.id, error.message()));
    }
}

#[test]
fn every_rejected_vector_is_rejected_for_the_declared_reason() {
    let manifest = manifest();
    for vector in &manifest.rejected {
        let rejection = rejection_for(&root().join(&vector.path));
        assert_eq!(
            rejection.class(),
            expected_class(&vector.id),
            "{} was rejected by the wrong layer: {}",
            vector.id,
            rejection.message()
        );
        if rejection.class() != RejectionClass::Loader {
            assert_eq!(
                rejection.class(),
                match surface(&vector.surface) {
                    Surface::Xdr => RejectionClass::Xdr,
                    Surface::Rpc => RejectionClass::Rpc,
                    Surface::Soroban => RejectionClass::Soroban,
                },
                "{} was rejected by a parser for the wrong surface: {}",
                vector.id,
                rejection.message()
            );
        }

        let message = rejection.message();
        for fragment in expected_fragments(&vector.error_code) {
            assert!(
                message.contains(fragment),
                "{} ({}) rejection must mention {fragment:?}: {message}",
                vector.id,
                vector.error_code
            );
        }
    }
}

#[test]
fn a_malformed_toml_vector_is_a_loader_parse_error_with_its_path() {
    let path = root().join("invalid/xdr-toml-syntax.toml");
    let error = parse_fixture_file(&path).expect_err("malformed TOML must not parse");
    assert!(matches!(error, FixtureError::Parse { .. }), "{error}");
    let rendered = error.to_string();
    assert!(rendered.contains("xdr-toml-syntax.toml"), "{rendered}");
}
