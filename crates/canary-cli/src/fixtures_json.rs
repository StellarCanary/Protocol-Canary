//! The machine-readable document `stellar-canary fixtures --format json`
//! prints (CA-02, prerequisite issue #440), including the `verification`
//! field CA-02 reserves for CF-04 evidence (issue #444).
//!
//! The shape is written out explicitly, like `canary_report::json`, so
//! no internal type change can silently change the document. The
//! reserved `sha256`, `payloads` and `packDigest` fields stay absent
//! until CF-02 lands; a field is never invented.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use canary_fixtures::{LoadedFixture, VerificationRecord};

/// The current inventory document version (`inventoryVersion`).
pub const INVENTORY_VERSION: u32 = 1;

/// The fixture file format the engine reads today.
pub const FIXTURE_FORMAT: u32 = 1;

/// The pack identity recorded in the inventory, when running over a fixture directory.
#[derive(Debug, Serialize)]
pub struct PackIdentity {
    /// SHA-256 digest of the fixture pack.
    pub pack_digest: String,
    /// Protocol version.
    pub protocol: u32,
    /// Registry version.
    pub registry_version: u32,
    /// Source of the pack (e.g., "directory").
    pub source: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureInventory {
    pub inventory_version: u32,
    pub tool_version: String,
    pub fixture_format: u32,
    pub protocol: u32,
    pub directory_found: bool,
    pub counts: InventoryCounts,
    /// Count of fixtures for every *other* protocol, keyed by the
    /// protocol number as a string (JSON object keys are strings).
    pub other_protocols: BTreeMap<String, usize>,
    /// One entry per fixture of `protocol`, sorted by id (UTF-8 bytes).
    pub fixtures: Vec<InventoryFixture>,
    /// Pack identity, recorded when running over a local fixture directory.
    pub pack: Option<PackIdentity>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryCounts {
    pub total: usize,
    pub xdr: usize,
    pub rpc: usize,
    pub soroban: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryFixture {
    pub id: String,
    pub protocol: u32,
    pub surface: String,
    pub category: String,
    pub description: String,
    /// Path of the fixture file relative to `--fixtures-dir`, with `/`
    /// separators on every platform.
    pub path: String,
    /// Sorted, lowercase kebab.
    pub required_capabilities: Vec<String>,
    /// Omitted, not `null`, when the file has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<String>,
    /// `rpc` fixtures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// `xdr` fixtures.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    /// `xdr` fixtures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// `soroban` fixtures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// The CF-04 verification record, copied as stored. Absent when the
    /// fixture has no record: no record means no field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification: Option<VerificationRecord>,
}

/// Builds the inventory for `protocol` from every loaded fixture in the
/// directory (of all protocols).
pub fn build(
    protocol: u32,
    directory_found: bool,
    fixtures_dir: &Path,
    loaded: &[LoadedFixture],
) -> FixtureInventory {
    let mut counts = InventoryCounts::default();
    let mut other_protocols: BTreeMap<String, usize> = BTreeMap::new();

    for fixture in loaded {
        if fixture.metadata.protocol.0 == protocol {
            counts.total += 1;
            match fixture.metadata.surface {
                canary_core::Surface::Xdr => counts.xdr += 1,
                canary_core::Surface::Rpc => counts.rpc += 1,
                canary_core::Surface::Soroban => counts.soroban += 1,
            }
        } else {
            *other_protocols
                .entry(fixture.metadata.protocol.0.to_string())
                .or_default() += 1;
        }
    }

    let mut fixtures: Vec<InventoryFixture> = loaded
        .iter()
        .filter(|fixture| fixture.metadata.protocol.0 == protocol)
        .map(|fixture| entry(fixture, fixtures_dir))
        .collect();
    fixtures.sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));

    let pack = if directory_found {
        let digest = match canary_fixtures::digest::pack_digest(fixtures_dir) {
            Ok(d) => d.digest,
            Err(_) => {
                // Do not abort the run when the pack is unusable: the check
                // still matters, but we cannot claim a digest we could not
                // compute. Omit identity rather than invent one.
                return FixtureInventory {
                    inventory_version: INVENTORY_VERSION,
                    tool_version: env!("CARGO_PKG_VERSION").to_string(),
                    fixture_format: FIXTURE_FORMAT,
                    protocol,
                    directory_found,
                    counts,
                    other_protocols,
                    pack: None,
                    fixtures,
                };
            }
        };

        Some(PackIdentity {
            pack_digest: digest,
            protocol,
            registry_version: 1,
            source: "directory".to_string(),
        })
    } else {
        None
    };

    FixtureInventory {
        inventory_version: INVENTORY_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        fixture_format: FIXTURE_FORMAT,
        protocol,
        directory_found,
        counts,
        other_protocols,
        pack,
        fixtures,
    }
}

fn entry(fixture: &LoadedFixture, fixtures_dir: &Path) -> InventoryFixture {
    let body_string = |key: &str| {
        fixture
            .body
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_string)
    };

    let mut required_capabilities: Vec<String> = fixture
        .metadata
        .required_capabilities
        .iter()
        .map(|capability| capability_name(capability).to_string())
        .collect();
    required_capabilities.sort();

    InventoryFixture {
        id: fixture.metadata.id.clone(),
        protocol: fixture.metadata.protocol.0,
        surface: fixture.metadata.surface.to_string(),
        category: fixture.metadata.category.clone(),
        description: fixture.metadata.description.clone(),
        path: relative_path(fixtures_dir, &fixture.source_path),
        required_capabilities,
        source_reference: fixture.metadata.source_reference.clone(),
        method: match fixture.metadata.surface {
            canary_core::Surface::Rpc => body_string("method"),
            _ => None,
        },
        type_name: match fixture.metadata.surface {
            canary_core::Surface::Xdr => body_string("type"),
            _ => None,
        },
        kind: match fixture.metadata.surface {
            canary_core::Surface::Xdr => body_string("kind"),
            _ => None,
        },
        function: match fixture.metadata.surface {
            canary_core::Surface::Soroban => body_string("function"),
            _ => None,
        },
        verification: fixture.verification.clone(),
    }
}

/// Renders `path` relative to `fixtures_dir` with `/` separators, never
/// absolute and never containing `..`. Falls back to the file name when
/// the path is not under the directory (it always is when the fixture
/// came from `load_directory`, so this only keeps the field valid).
fn relative_path(fixtures_dir: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(fixtures_dir).unwrap_or(path);
    let mut parts = Vec::new();
    for component in relative.components() {
        if let std::path::Component::Normal(part) = component {
            if let Some(text) = part.to_str() {
                parts.push(text.to_string());
            }
        }
    }
    if parts.is_empty() {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        parts.join("/")
    }
}

/// The lowercase kebab name of a capability, as CA-02 spells them.
pub fn capability_name(capability: &canary_core::Capability) -> &'static str {
    match capability {
        canary_core::Capability::SorobanContract => "soroban-contract",
        canary_core::Capability::RpcClient => "rpc-client",
        canary_core::Capability::StellarSdkDependency => "stellar-sdk-dependency",
        canary_core::Capability::WasmArtifact => "wasm-artifact",
        canary_core::Capability::RawLedgerAccess => "raw-ledger-access",
    }
}

/// Renders an inventory as CA-02 section 3 requires: UTF-8, two-space
/// indent, LF, and a final newline.
pub fn render(inventory: &FixtureInventory) -> String {
    let json = serde_json::to_string_pretty(inventory)
        .expect("a FixtureInventory always serializes: it contains no maps with non-string keys");
    format!("{json}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use canary_core::{Capability, FixtureMetadata, ProtocolVersion, Surface};
    use canary_fixtures::VerificationMethod;

    fn fixture(id: &str, protocol: u32, surface: Surface, verification: bool) -> LoadedFixture {
        fn body_entry(table: &mut toml::map::Map<String, toml::Value>, key: &str, value: &str) {
            table.insert(key.to_string(), toml::Value::String(value.to_string()));
        }
        let mut table = toml::map::Map::new();
        match surface {
            Surface::Rpc => body_entry(&mut table, "method", "get-network"),
            Surface::Xdr => {
                body_entry(&mut table, "type", "StellarValue");
                body_entry(&mut table, "kind", "decode-success");
            }
            Surface::Soroban => body_entry(&mut table, "function", "name"),
        }
        let body = toml::Value::Table(table);
        let record = verification.then(|| VerificationRecord {
            verification_version: 1,
            fixture_id: id.to_string(),
            fixture_sha256: None,
            method: VerificationMethod::Structural,
            checked_at: "2026-09-28T07:20:00Z".to_string(),
            network: None,
            endpoint_host: None,
            verifier: None,
            evidence_ref: None,
            outcome: canary_fixtures::VerificationOutcome::Matched,
        });
        LoadedFixture {
            metadata: FixtureMetadata {
                id: id.to_string(),
                protocol: ProtocolVersion(protocol),
                surface,
                category: "test".to_string(),
                description: "test".to_string(),
                source_reference: None,
                required_capabilities: vec![Capability::StellarSdkDependency],
                content_digest: String::new(),
            },
            source_path: Path::new("fixtures").join(format!("{id}.toml")),
            input_file: None,
            expected_file: None,
            body,
            verification: record,
        }
    }

    #[test]
    fn renders_the_ca02_top_level_shape() {
        let inventory = build(
            28,
            true,
            Path::new("fixtures"),
            &[
                fixture("p28-xdr-1", 28, Surface::Xdr, false),
                fixture("p28-rpc-1", 28, Surface::Rpc, true),
                fixture("p28-soroban-1", 28, Surface::Soroban, false),
                fixture("p27-xdr-old", 27, Surface::Xdr, false),
            ],
        );
        let value: serde_json::Value = serde_json::from_str(&render(&inventory)).unwrap();

        assert_eq!(value["inventoryVersion"], 1);
        assert_eq!(value["fixtureFormat"], 1);
        assert_eq!(value["protocol"], 28);
        assert_eq!(value["directoryFound"], true);
        assert_eq!(value["counts"]["total"], 3);
        assert_eq!(value["counts"]["xdr"], 1);
        assert_eq!(value["counts"]["rpc"], 1);
        assert_eq!(value["counts"]["soroban"], 1);
        assert_eq!(value["otherProtocols"]["27"], 1);
        // Sorted by id, not by surface or file name.
        let ids: Vec<&str> = value["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["p28-rpc-1", "p28-soroban-1", "p28-xdr-1"]);
    }

    #[test]
    fn surfaces_carry_their_documented_fields() {
        let inventory = build(
            28,
            true,
            Path::new("fixtures"),
            &[
                fixture("p28-xdr-1", 28, Surface::Xdr, false),
                fixture("p28-rpc-1", 28, Surface::Rpc, false),
                fixture("p28-soroban-1", 28, Surface::Soroban, false),
            ],
        );
        let value: serde_json::Value = serde_json::from_str(&render(&inventory)).unwrap();
        let entries = value["fixtures"].as_array().unwrap();

        let xdr = &entries[2];
        assert_eq!(xdr["type"], "StellarValue");
        assert_eq!(xdr["kind"], "decode-success");
        assert!(xdr.get("method").is_none());
        assert!(xdr.get("verification").is_none());

        let rpc = &entries[0];
        assert_eq!(rpc["method"], "get-network");
        assert!(rpc.get("type").is_none());

        let soroban = &entries[1];
        assert_eq!(soroban["function"], "name");
    }

    #[test]
    fn the_verification_record_is_copied_as_stored() {
        let inventory = build(
            28,
            true,
            Path::new("fixtures"),
            &[fixture("p28-rpc-1", 28, Surface::Rpc, true)],
        );
        let value: serde_json::Value = serde_json::from_str(&render(&inventory)).unwrap();
        let verification = &value["fixtures"][0]["verification"];
        assert_eq!(verification["verificationVersion"], 1);
        assert_eq!(verification["fixtureId"], "p28-rpc-1");
        assert_eq!(verification["method"], "structural");
        assert_eq!(verification["checkedAt"], "2026-09-28T07:20:00Z");
        assert_eq!(verification["outcome"], "matched");
    }

    #[test]
    fn a_fixture_without_a_record_omits_the_field_entirely() {
        let inventory = build(
            28,
            true,
            Path::new("fixtures"),
            &[fixture("p28-rpc-1", 28, Surface::Rpc, false)],
        );
        let value: serde_json::Value = serde_json::from_str(&render(&inventory)).unwrap();
        assert!(value["fixtures"][0].get("verification").is_none());
    }

    #[test]
    fn required_capabilities_are_sorted_kebab_names() {
        let mut fixture = fixture("p28-xdr-1", 28, Surface::Xdr, false);
        fixture.metadata.required_capabilities =
            vec![Capability::WasmArtifact, Capability::SorobanContract];
        let inventory = build(28, true, Path::new("fixtures"), &[fixture]);
        let value: serde_json::Value = serde_json::from_str(&render(&inventory)).unwrap();
        assert_eq!(
            value["fixtures"][0]["requiredCapabilities"],
            serde_json::json!(["soroban-contract", "wasm-artifact"])
        );
    }

    #[test]
    fn an_empty_or_missing_directory_is_a_valid_answer() {
        let inventory = build(28, false, Path::new("fixtures"), &[]);
        let value: serde_json::Value = serde_json::from_str(&render(&inventory)).unwrap();
        assert_eq!(value["directoryFound"], false);
        assert_eq!(value["fixtures"], serde_json::json!([]));
        assert_eq!(value["otherProtocols"], serde_json::json!({}));
    }

    #[test]
    fn paths_use_slashes_and_stay_relative() {
        let inventory = build(
            28,
            true,
            Path::new("fixtures"),
            &[fixture("p28-xdr-1", 28, Surface::Xdr, false)],
        );
        let value: serde_json::Value = serde_json::from_str(&render(&inventory)).unwrap();
        assert_eq!(value["fixtures"][0]["path"], "p28-xdr-1.toml");
    }

    #[test]
    fn renders_two_space_indent_with_a_final_newline() {
        let inventory = build(28, true, Path::new("fixtures"), &[]);
        let rendered = render(&inventory);
        assert!(
            rendered.contains("\n  \"inventoryVersion\": 1"),
            "{rendered}"
        );
        assert!(rendered.ends_with("}\n"), "{rendered}");
    }
}
