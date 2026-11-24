//! Capability parity between the Rust engine and the fixture validator
//! (E-114, issue #512).
//!
//! The engine's [`canary_core::Capability`] enum and the canonical capability
//! vector at `tests/fixtures/capabilities.json` must always describe the same
//! vocabulary: exactly the 5 capability strings approved in
//! ProtocolCanary-Fixtures#288 (F-032) — `soroban-contract`, `rpc-client`,
//! `stellar-sdk-dependency`, `wasm-artifact` and `raw-ledger-access` — which
//! is also the `CAPABILITIES` set in ProtocolCanary-Fixtures'
//! `tools/validate/validate.py` and the `required_capabilities` enum in its
//! `schemas/fixture-v1.schema.json` (CF-07 section 9).
//!
//! It also pins capability parity *across the members of a Cargo workspace*
//! (issue #754): when a virtual manifest groups several contracts and
//! services, every capability must be attributed to the member crate that
//! actually declares the triggering dependency, never blended across the
//! whole workspace.
//!
//! Run with `cargo test -p canary-project --test capability_parity`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_core::Capability;
use canary_project::{
    detect_capabilities, detect_details, scan_cargo_workspace, DetailedDetection, DetectionSignals,
    ProjectManifest,
};
use serde_json::Value;

/// The canonical capability vector, loaded from the repository fixture.
const CAPABILITIES_JSON: &str = include_str!("../../../tests/fixtures/capabilities.json");

/// The 5 capability strings approved by ProtocolCanary-Fixtures#288 (F-032).
const APPROVED_CAPABILITIES: &[&str] = &[
    "soroban-contract",
    "rpc-client",
    "stellar-sdk-dependency",
    "wasm-artifact",
    "raw-ledger-access",
];

/// Reads the canonical capability vector out of its JSON representation.
///
/// A bare JSON array of strings and an object carrying the
/// `required_capabilities` field — the exact field name the fixture schema
/// mandates — are both accepted, so the parity test keeps working whichever
/// shape ProtocolCanary-Fixtures#288 settles on.
///
/// # Panics
///
/// Panics when the file is not valid JSON, does not hold a capability array,
/// or holds an entry that is not a string; a fixture vector the test cannot
/// read must never silently compare as empty.
fn parse_capability_vector(json: &str) -> Vec<String> {
    let value: Value = serde_json::from_str(json)
        .unwrap_or_else(|error| panic!("capabilities.json is not valid JSON: {error}"));
    let vector = match value {
        Value::Array(_) => value,
        Value::Object(mut fields) => fields.remove("required_capabilities").unwrap_or_else(|| {
            panic!("capabilities.json object must carry a `required_capabilities` array")
        }),
        other => panic!("capabilities.json must be an array or an object, got {other}"),
    };
    let entries = vector
        .as_array()
        .unwrap_or_else(|| panic!("capability vector must be a JSON array, got {vector}"));
    let mut tags: Vec<String> = entries
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .unwrap_or_else(|| panic!("capability entries must be strings, got {entry}"))
                .to_string()
        })
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

/// The kebab-case wire name a capability serializes to inside fixtures.
fn capability_tag(capability: &Capability) -> String {
    serde_json::to_string(capability)
        .expect("Capability serializes to a plain string")
        .trim_matches('"')
        .to_string()
}

/// Every capability declared by the engine, as fixture-facing tags.
fn engine_tags() -> Vec<String> {
    Capability::all().iter().map(capability_tag).collect()
}

/// Compares two capability tag sets and reports every difference.
///
/// Tags present in `engine` but absent from `fixture` are *missing* from the
/// fixture vector; tags present in `fixture` but absent from `engine` are
/// *extra* — declared without a matching engine variant. Either direction
/// breaks parity, and the error names the offending tags.
fn assert_parity(engine: &[String], fixture: &[String]) -> Result<(), String> {
    let engine: BTreeSet<&str> = engine.iter().map(String::as_str).collect();
    let fixture: BTreeSet<&str> = fixture.iter().map(String::as_str).collect();

    let missing: Vec<&str> = engine.difference(&fixture).copied().collect();
    let extra: Vec<&str> = fixture.difference(&engine).copied().collect();

    if missing.is_empty() && extra.is_empty() {
        return Ok(());
    }
    Err(format!(
        "capability sets diverge: {missing:?} declared by the engine but missing from the \
         fixture vector, {extra:?} present in the fixture vector but undeclared by the engine"
    ))
}

#[test]
fn fixture_vector_holds_exactly_the_approved_capability_strings() {
    let vector = parse_capability_vector(CAPABILITIES_JSON);
    let expected: BTreeSet<String> = APPROVED_CAPABILITIES
        .iter()
        .map(|tag| (*tag).to_string())
        .collect();

    assert_eq!(
        vector.iter().cloned().collect::<BTreeSet<_>>(),
        expected,
        "tests/fixtures/capabilities.json must hold exactly the 5 approved \
         capability strings from ProtocolCanary-Fixtures#288"
    );
    assert_eq!(vector.len(), APPROVED_CAPABILITIES.len());
}

#[test]
fn engine_capability_set_matches_fixture_vector() {
    let fixture = parse_capability_vector(CAPABILITIES_JSON);

    if let Err(report) = assert_parity(&engine_tags(), &fixture) {
        panic!("{report}");
    }
}

#[test]
fn engine_serializes_to_kebab_case_fixture_tags() {
    let tags = engine_tags();

    assert_eq!(
        tags,
        vec![
            "soroban-contract",
            "rpc-client",
            "stellar-sdk-dependency",
            "wasm-artifact",
            "raw-ledger-access",
        ],
        "Capability must serialize to the kebab-case tags the fixture validator accepts"
    );
}

#[test]
fn every_capability_variant_maps_to_an_approved_tag() {
    /// Exhaustive over [`Capability`]: adding a variant fails compilation here
    /// until its fixture tag is spelled out, which points the author at the
    /// `Capability::all()` list and `tests/fixtures/capabilities.json` that
    /// must grow with it.
    fn fixture_tag(capability: &Capability) -> &'static str {
        match capability {
            Capability::SorobanContract => "soroban-contract",
            Capability::RpcClient => "rpc-client",
            Capability::StellarSdkDependency => "stellar-sdk-dependency",
            Capability::WasmArtifact => "wasm-artifact",
            Capability::RawLedgerAccess => "raw-ledger-access",
        }
    }

    for capability in Capability::all() {
        assert!(
            APPROVED_CAPABILITIES.contains(&fixture_tag(capability)),
            "{capability:?} serializes to a tag ProtocolCanary-Fixtures#288 has not approved"
        );
    }
}

#[test]
fn detector_only_emits_declared_capabilities() {
    let signals = DetectionSignals {
        manifests: vec![ProjectManifest {
            dependency_names: vec![
                "soroban-sdk".to_string(),
                "stellar-sdk".to_string(),
                "stellar-rpc-client".to_string(),
            ],
        }],
        has_stellar_toml: true,
        has_wasm_artifact: true,
    };
    let declared: BTreeSet<String> = Capability::all().iter().map(capability_tag).collect();

    for capability in detect_capabilities(&signals) {
        assert!(
            declared.contains(&capability_tag(&capability)),
            "detector emitted {capability:?}, which Capability::all() does not declare"
        );
    }
}

#[test]
fn parity_detects_a_capability_missing_from_the_fixture_vector() {
    let mut truncated = parse_capability_vector(CAPABILITIES_JSON);
    truncated.retain(|tag| tag != "raw-ledger-access");

    let report = assert_parity(&engine_tags(), &truncated)
        .expect_err("dropping a capability from the fixture vector must fail parity");
    assert!(report.contains("raw-ledger-access"));
    assert!(report.contains("missing from the fixture vector"));
}

#[test]
fn parity_detects_an_undeclared_capability_added_to_the_fixture_vector() {
    let mut extended = parse_capability_vector(CAPABILITIES_JSON);
    extended.push("fake-cap".to_string());

    let report = assert_parity(&engine_tags(), &extended)
        .expect_err("an extra capability in the fixture vector must fail parity");
    assert!(report.contains("fake-cap"));
    assert!(report.contains("undeclared by the engine"));
}

#[test]
fn parity_detects_a_synthetic_engine_capability() {
    let fixture = parse_capability_vector(CAPABILITIES_JSON);
    let mut extended = engine_tags();
    extended.push("fake-cap".to_string());

    let report = assert_parity(&extended, &fixture)
        .expect_err("an extra capability on the engine side must fail parity");
    assert!(report.contains("fake-cap"));
    assert!(report.contains("missing from the fixture vector"));
}

#[test]
fn parity_detects_an_empty_fixture_vector() {
    let report = assert_parity(&engine_tags(), &[])
        .expect_err("an empty fixture vector must fail parity against the engine");
    for tag in APPROVED_CAPABILITIES {
        assert!(
            report.contains(tag),
            "{tag} missing from the report: {report}"
        );
    }
}

#[test]
#[should_panic(expected = "`required_capabilities`")]
fn parser_rejects_an_object_without_required_capabilities() {
    parse_capability_vector(r#"{"capabilities": ["soroban-contract"]}"#);
}

#[test]
#[should_panic(expected = "must be strings")]
fn parser_rejects_non_string_entries() {
    parse_capability_vector(r#"["soroban-contract", 7]"#);
}

// ---------------------------------------------------------------------------
// Workspace package parity (issue #754)
// ---------------------------------------------------------------------------

/// A scratch workspace directory under [`std::env::temp_dir`], removed on
/// drop. The name combines the process id and a per-process counter so tests
/// the harness runs in parallel never share (and delete) one another's tree.
struct ScratchWorkspace(PathBuf);

impl ScratchWorkspace {
    fn new(name: &str) -> ScratchWorkspace {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "canary-parity-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        ScratchWorkspace(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `text` to the `/`-separated `relative` path, creating parents.
    fn write(&self, relative: &str, text: &str) {
        let path = relative
            .split('/')
            .fold(self.0.clone(), |path, segment| path.join(segment));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Writes a member crate's `Cargo.toml` declaring `dependencies`.
    fn member(&self, dir: &str, package: &str, dependencies: &[&str]) {
        let mut manifest =
            format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\n\n[dependencies]\n");
        for dependency in dependencies {
            manifest.push_str(&format!("{dependency} = \"1\"\n"));
        }
        self.write(&format!("{dir}/Cargo.toml"), &manifest);
    }
}

impl Drop for ScratchWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One member of the disjoint-capability fixture: where it lives, its
/// package name, what it depends on, and the capabilities that alone must be
/// attributed to it.
struct MemberSpec {
    dir: &'static str,
    package: &'static str,
    dependencies: &'static [&'static str],
    capabilities: &'static [Capability],
}

/// Four members whose capability sets do not overlap: a Soroban contract, an
/// RPC-consuming service, an XDR helper, and a plain crate with no Stellar
/// surface at all. Each dependency triggers exactly one capability, so any
/// blending between members shows up as an extra or missing tag.
const DISJOINT_MEMBERS: &[MemberSpec] = &[
    MemberSpec {
        dir: "contracts/token",
        package: "token-contract",
        dependencies: &["soroban-sdk"],
        capabilities: &[Capability::SorobanContract],
    },
    MemberSpec {
        dir: "services/indexer",
        package: "indexer-service",
        dependencies: &["stellar-rpc-client", "tokio"],
        capabilities: &[Capability::RpcClient],
    },
    MemberSpec {
        dir: "tools/xdr-helper",
        package: "xdr-helper",
        dependencies: &["stellar-xdr", "serde"],
        capabilities: &[Capability::StellarSdkDependency],
    },
    MemberSpec {
        dir: "crates/plain",
        package: "plain-utils",
        dependencies: &["serde", "thiserror"],
        capabilities: &[],
    },
];

/// Builds the disjoint fixture behind a virtual manifest (a root
/// `Cargo.toml` with a `[workspace]` table and no `[package]` or
/// dependencies of its own).
fn disjoint_workspace(name: &str) -> ScratchWorkspace {
    let scratch = ScratchWorkspace::new(name);
    let members: Vec<String> = DISJOINT_MEMBERS
        .iter()
        .map(|member| format!("\"{}\"", member.dir))
        .collect();
    scratch.write(
        "Cargo.toml",
        &format!(
            "[workspace]\nresolver = \"2\"\nmembers = [{}]\n",
            members.join(", ")
        ),
    );
    for member in DISJOINT_MEMBERS {
        scratch.member(member.dir, member.package, member.dependencies);
    }
    scratch
}

/// The member directory that owns an evidence record's manifest path
/// (`contracts/token/Cargo.toml` -> `contracts/token`).
fn owning_member(manifest: &str) -> &str {
    manifest
        .strip_suffix("/Cargo.toml")
        .unwrap_or_else(|| panic!("evidence manifest {manifest:?} is not a member Cargo.toml"))
}

/// Capabilities grouped by the workspace member whose evidence produced
/// them, built purely from the detector's evidence records.
fn capabilities_by_member(details: &DetailedDetection) -> BTreeMap<String, BTreeSet<String>> {
    let mut by_member: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in &details.capability_evidence {
        for evidence in &entry.evidence {
            by_member
                .entry(owning_member(&evidence.manifest).to_string())
                .or_default()
                .insert(capability_tag(&entry.capability));
        }
    }
    by_member
}

/// The `[package] name` declared by the manifest at `relative` under `root`.
fn package_name(root: &Path, relative: &str) -> String {
    let path = relative
        .split('/')
        .fold(root.to_path_buf(), |path, segment| path.join(segment));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    let value: toml::Value = toml::from_str(&text).expect("member manifest is valid TOML");
    value["package"]["name"]
        .as_str()
        .expect("member manifest declares [package] name")
        .to_string()
}

fn tags(capabilities: &[Capability]) -> BTreeSet<String> {
    capabilities.iter().map(capability_tag).collect()
}

#[test]
fn workspace_members_with_disjoint_capabilities_stay_partitioned() {
    let scratch = disjoint_workspace("disjoint");
    let details = detect_details(scratch.path());

    let expected: BTreeMap<String, BTreeSet<String>> = DISJOINT_MEMBERS
        .iter()
        .filter(|member| !member.capabilities.is_empty())
        .map(|member| (member.dir.to_string(), tags(member.capabilities)))
        .collect();

    assert_eq!(
        capabilities_by_member(&details),
        expected,
        "each member must own exactly the capabilities its own dependencies trigger"
    );

    // Every member was scanned, including the one with no Stellar surface.
    let mut dirs: Vec<&str> = DISJOINT_MEMBERS.iter().map(|m| m.dir).collect();
    dirs.sort_unstable();
    assert_eq!(details.workspace_members, dirs);
}

#[test]
fn no_capability_evidence_leaks_across_workspace_members() {
    let scratch = disjoint_workspace("no-leak");
    let details = detect_details(scratch.path());

    for entry in &details.capability_evidence {
        let owners: BTreeSet<&str> = entry
            .evidence
            .iter()
            .map(|evidence| owning_member(&evidence.manifest))
            .collect();
        let expected: BTreeSet<&str> = DISJOINT_MEMBERS
            .iter()
            .filter(|member| member.capabilities.contains(&entry.capability))
            .map(|member| member.dir)
            .collect();
        assert_eq!(
            owners, expected,
            "{:?} evidence must come only from the member that declares it",
            entry.capability
        );

        for evidence in &entry.evidence {
            let member = DISJOINT_MEMBERS
                .iter()
                .find(|member| member.dir == owning_member(&evidence.manifest))
                .expect("evidence points at a declared member");
            let dependency = evidence
                .dependency
                .as_deref()
                .expect("manifest evidence names its dependency");
            assert!(
                member.dependencies.contains(&dependency),
                "{dependency} is attributed to {} but that member does not declare it",
                member.dir
            );
            assert_eq!(evidence.ecosystem, "cargo");
            assert_eq!(evidence.section.as_deref(), Some("dependencies"));
            assert_eq!(evidence.source_file, evidence.manifest);
        }
    }

    // The member without a Stellar surface owns no evidence at all, and the
    // virtual root manifest never contributes evidence of its own.
    assert!(details
        .capability_evidence
        .iter()
        .flat_map(|entry| &entry.evidence)
        .all(|e| e.manifest != "crates/plain/Cargo.toml" && e.manifest != "Cargo.toml"));
}

#[test]
fn evidence_records_name_the_package_of_their_owning_member() {
    let scratch = disjoint_workspace("package-names");
    let details = detect_details(scratch.path());

    let mut seen = BTreeSet::new();
    for entry in &details.capability_evidence {
        for evidence in &entry.evidence {
            let package = package_name(scratch.path(), &evidence.manifest);
            let member = DISJOINT_MEMBERS
                .iter()
                .find(|member| member.package == package)
                .unwrap_or_else(|| panic!("unexpected package {package:?}"));
            assert_eq!(
                owning_member(&evidence.manifest),
                member.dir,
                "evidence manifest and package name must agree"
            );
            assert!(
                member.capabilities.contains(&entry.capability),
                "{:?} evidence names package {package:?}, which does not own that capability",
                entry.capability
            );
            seen.insert(package);
        }
    }

    let expected: BTreeSet<String> = DISJOINT_MEMBERS
        .iter()
        .filter(|member| !member.capabilities.is_empty())
        .map(|member| member.package.to_string())
        .collect();
    assert_eq!(seen, expected, "every Stellar member is named by evidence");
}

#[test]
fn per_member_detection_matches_workspace_attribution() {
    // Parity between the two entry points: running the capability detector
    // on each member's manifest alone must give the same per-member sets the
    // workspace-wide detection attributes to that member, and their union
    // must be the workspace's declared capabilities.
    let scratch = disjoint_workspace("per-member");
    let details = detect_details(scratch.path());
    let attributed = capabilities_by_member(&details);

    let scan = scan_cargo_workspace(scratch.path());
    assert!(scan.diagnostics.is_empty(), "{:?}", scan.diagnostics);
    assert_eq!(scan.members.len(), DISJOINT_MEMBERS.len());

    let mut union = BTreeSet::new();
    for member in &scan.members {
        let alone = tags(&detect_capabilities(&DetectionSignals {
            manifests: vec![member.manifest.clone()],
            ..Default::default()
        }));
        assert_eq!(
            attributed.get(&member.path).cloned().unwrap_or_default(),
            alone,
            "member {} must be attributed exactly what it detects on its own",
            member.path
        );
        union.extend(alone);
    }

    assert_eq!(tags(&details.context.capabilities), union);
}

#[test]
fn a_capability_shared_by_two_members_keeps_one_record_per_member() {
    let scratch = ScratchWorkspace::new("shared");
    scratch.write("Cargo.toml", "[workspace]\nmembers = [\"contracts/*\"]\n");
    scratch.member("contracts/escrow", "escrow", &["soroban-sdk"]);
    scratch.member("contracts/vault", "vault", &["soroban-sdk", "stellar-xdr"]);

    let details = detect_details(scratch.path());
    let soroban = details
        .capability_evidence
        .iter()
        .find(|entry| entry.capability == Capability::SorobanContract)
        .expect("soroban evidence");
    let manifests: Vec<&str> = soroban
        .evidence
        .iter()
        .map(|e| e.manifest.as_str())
        .collect();
    assert_eq!(
        manifests,
        vec!["contracts/escrow/Cargo.toml", "contracts/vault/Cargo.toml"],
        "a shared capability is reported once per owning member, not merged"
    );

    let sdk = details
        .capability_evidence
        .iter()
        .find(|entry| entry.capability == Capability::StellarSdkDependency)
        .expect("stellar sdk evidence");
    assert_eq!(sdk.evidence.len(), 1);
    assert_eq!(sdk.evidence[0].manifest, "contracts/vault/Cargo.toml");
    assert_eq!(
        package_name(scratch.path(), &sdk.evidence[0].manifest),
        "vault",
        "the SDK capability belongs to vault only, not to its sibling escrow"
    );
}

#[test]
fn an_excluded_member_contributes_no_capability_or_evidence() {
    let scratch = ScratchWorkspace::new("excluded");
    scratch.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/legacy-rpc\"]\n",
    );
    scratch.member("crates/contract", "contract", &["soroban-sdk"]);
    scratch.member("crates/legacy-rpc", "legacy-rpc", &["stellar-rpc-client"]);

    let details = detect_details(scratch.path());
    assert_eq!(details.workspace_members, vec!["crates/contract"]);
    assert_eq!(
        details.context.capabilities,
        vec![Capability::SorobanContract]
    );
    assert!(details
        .capability_evidence
        .iter()
        .flat_map(|entry| &entry.evidence)
        .all(|e| !e.manifest.starts_with("crates/legacy-rpc/")));
}
