//! The machine-readable document `stellar-canary inspect --format json`
//! prints (CA-03, issue #453), shared by the parity tests that assert
//! the plan equals what `check` executes (issue #485).
//!
//! The shape is written out explicitly, like `canary_report::json`, so a
//! change to an internal type can never silently change the document.
//! Conforms to CA-03, CF-07, and CA-04.

use serde::Serialize;

use canary_core::Capability;

/// The current inspect document version (`inspectVersion`).
pub const INSPECT_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectDocument {
    pub inspect_version: u32,
    pub tool_version: String,
    /// Always `true`: `inspect` never opens a network connection. A
    /// consumer can assert it.
    pub offline: bool,
    pub project: InspectProject,
    pub config: InspectConfig,
    pub target: InspectTarget,
    pub fixtures: InspectFixtures,
    pub plan: InspectPlan,
    pub workspace: InspectWorkspace,
    pub fingerprint: InspectFingerprint,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectProject {
    /// The selected root relative to the directory the command was
    /// started in. Never absolute.
    pub root: String,
    pub root_selected_by: String,
    #[serde(rename = "type")]
    pub project_type: String,
    /// `detected` or `config`.
    pub type_source: String,
    pub detected_type: String,
    /// Sorted, lowercase kebab.
    pub capabilities: Vec<String>,
    pub workspace: InspectWorkspace,
    pub detection: InspectDetection,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectWorkspace {
    pub members: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectFingerprint {
    pub fingerprint_version: u32,
    pub detector: u64,
    pub digest: String,
    pub inputs: usize,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectDetection {
    pub evidence_version: u32,
    pub root: String,
    pub capabilities: Vec<InspectCapabilityEvidence>,
    pub inspected: Vec<String>,
    pub diagnostics: Vec<InspectDiagnostic>,
    pub project_type: InspectProjectTypeEvidence,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectCapabilityEvidence {
    pub capability: String,
    pub evidence: Vec<InspectEvidenceItem>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectEvidenceItem {
    pub manifest: String,
    pub ecosystem: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectDiagnostic {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectProjectTypeEvidence {
    pub value: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_value: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectConfig {
    pub found: bool,
    /// The configuration file's path relative to the root, with `/`. `null`
    /// when no file was read, or when an explicit `--config` points
    /// outside the root.
    pub path: Option<String>,
    pub version: u32,
    pub protocol: u32,
    /// `auto` or an explicit type name.
    pub project_type: String,
    pub tests: InspectTests,
    pub policy: InspectPolicy,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectTests {
    pub xdr: bool,
    pub rpc: bool,
    pub soroban: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectPolicy {
    pub warnings_are_failures: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectTarget {
    pub protocol: u32,
    /// `flag`, `config` or `default`.
    pub source: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectFixtures {
    pub directory_found: bool,
    pub loaded: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectPlan {
    /// `{id, surface}` for every fixture `check` would execute, in the
    /// order `check` executes them.
    pub would_run: Vec<PlannedFixture>,
    /// `{id, surface, code, reason}` for every fixture left out, in the
    /// order `check` records them.
    pub skipped: Vec<PlannedSkip>,
    pub check_would_refuse: bool,
    /// The explanation `check` would print for an empty run, or `null`.
    pub refusal: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFixture {
    pub id: String,
    pub surface: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSkip {
    pub id: String,
    pub surface: String,
    /// `protocol-mismatch`, `surface-disabled` or `missing-capability`.
    pub code: String,
    pub reason: String,
}

/// Renders a document as CA-03 section 3 requires: UTF-8, two-space
/// indent, and a final newline.
pub fn render(document: &InspectDocument) -> String {
    let json = serde_json::to_string_pretty(document).expect(
        "an InspectDocument always serializes: it contains no maps and no non-finite numbers",
    );
    format!("{json}\n")
}

/// The lowercase kebab name of a capability, sorted into
/// [`InspectProject::capabilities`].
pub fn capability_name(capability: &Capability) -> &'static str {
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

    fn example() -> InspectDocument {
        InspectDocument {
            inspect_version: INSPECT_VERSION,
            tool_version: "0.0.0-test".to_string(),
            offline: true,
            project: InspectProject {
                root: ".".to_string(),
                root_selected_by: "current-directory".to_string(),
                project_type: "soroban".to_string(),
                type_source: "detected".to_string(),
                detected_type: "soroban".to_string(),
                capabilities: vec!["soroban-contract".to_string()],
                workspace: InspectWorkspace {
                    members: vec!["contracts/token".to_string()],
                },
                detection: InspectDetection {
                    evidence_version: 1,
                    root: ".".to_string(),
                    capabilities: vec![InspectCapabilityEvidence {
                        capability: "soroban-contract".to_string(),
                        evidence: vec![InspectEvidenceItem {
                            manifest: "contracts/token/Cargo.toml".to_string(),
                            ecosystem: "cargo".to_string(),
                            dependency: Some("soroban-sdk".to_string()),
                            section: Some("dependencies".to_string()),
                        }],
                    }],
                    inspected: vec![
                        "Cargo.toml".to_string(),
                        "contracts/token/Cargo.toml".to_string(),
                    ],
                    diagnostics: vec![],
                    project_type: InspectProjectTypeEvidence {
                        value: "soroban".to_string(),
                        source: "detected".to_string(),
                        detected_value: None,
                    },
                },
            },
            config: InspectConfig {
                found: false,
                path: None,
                version: 1,
                protocol: 28,
                project_type: "auto".to_string(),
                tests: InspectTests {
                    xdr: true,
                    rpc: true,
                    soroban: true,
                },
                policy: InspectPolicy {
                    warnings_are_failures: false,
                },
            },
            target: InspectTarget {
                protocol: 28,
                source: "default".to_string(),
            },
            fixtures: InspectFixtures {
                directory_found: false,
                loaded: 0,
            },
            plan: InspectPlan {
                would_run: vec![PlannedFixture {
                    id: "p28-rpc-get-network".to_string(),
                    surface: "rpc".to_string(),
                }],
                skipped: vec![PlannedSkip {
                    id: "p27-xdr-old".to_string(),
                    surface: "xdr".to_string(),
                    code: "protocol-mismatch".to_string(),
                    reason: "fixture targets protocol 27, this run targets protocol 28".to_string(),
                }],
                check_would_refuse: false,
                refusal: None,
            },
            workspace: InspectWorkspace {
                members: vec!["contracts/token".to_string()],
            },
            fingerprint: InspectFingerprint {
                fingerprint_version: 1,
                detector: 1,
                digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
                inputs: 2,
            },
        }
    }

    #[test]
    fn renders_the_ca03_top_level_shape() {
        let value: serde_json::Value = serde_json::from_str(&render(&example())).unwrap();
        assert_eq!(value["inspectVersion"], 1);
        assert_eq!(value["offline"], true);
        assert_eq!(value["project"]["root"], ".");
        assert_eq!(value["project"]["rootSelectedBy"], "current-directory");
        assert_eq!(value["project"]["type"], "soroban");
        assert_eq!(value["project"]["typeSource"], "detected");
        assert_eq!(value["project"]["detectedType"], "soroban");
        assert_eq!(
            value["project"]["workspace"]["members"][0],
            "contracts/token"
        );
        assert_eq!(value["project"]["detection"]["evidenceVersion"], 1);
        assert_eq!(
            value["project"]["detection"]["capabilities"][0]["capability"],
            "soroban-contract"
        );
        assert_eq!(value["config"]["found"], false);
        assert_eq!(value["config"]["path"], serde_json::Value::Null);
        assert_eq!(value["target"]["source"], "default");
        assert_eq!(value["fixtures"]["directoryFound"], false);
        assert_eq!(value["plan"]["wouldRun"][0]["id"], "p28-rpc-get-network");
        assert_eq!(value["plan"]["wouldRun"][0]["surface"], "rpc");
        assert_eq!(value["plan"]["skipped"][0]["code"], "protocol-mismatch");
        assert_eq!(value["plan"]["checkWouldRefuse"], false);
        assert_eq!(value["plan"]["refusal"], serde_json::Value::Null);
        assert_eq!(value["workspace"]["members"][0], "contracts/token");
        assert_eq!(value["fingerprint"]["fingerprintVersion"], 1);
        assert_eq!(value["fingerprint"]["detector"], 1);
        assert_eq!(
            value["fingerprint"]["digest"],
            "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(value["fingerprint"]["inputs"], 2);
    }

    #[test]
    fn renders_two_space_indent_with_a_final_newline() {
        let rendered = render(&example());
        assert!(rendered.contains("\n  \"inspectVersion\": 1"), "{rendered}");
        assert!(rendered.ends_with("}\n"), "{rendered}");
        assert!(!rendered.contains("\r"), "{rendered}");
    }

    #[test]
    fn an_empty_run_carries_the_refusal_text() {
        let mut document = example();
        document.plan.would_run.clear();
        document.plan.check_would_refuse = true;
        document.plan.refusal =
            Some("no checks ran: no fixture files (*.toml) were found in fixtures.".to_string());

        let value: serde_json::Value = serde_json::from_str(&render(&document)).unwrap();
        assert_eq!(value["plan"]["wouldRun"], serde_json::json!([]));
        assert_eq!(value["plan"]["checkWouldRefuse"], true);
        assert!(value["plan"]["refusal"]
            .as_str()
            .unwrap()
            .contains("no checks ran"));
    }

    #[test]
    fn capability_names_use_the_documented_kebab_spelling() {
        assert_eq!(
            capability_name(&Capability::SorobanContract),
            "soroban-contract"
        );
        assert_eq!(capability_name(&Capability::RpcClient), "rpc-client");
        assert_eq!(
            capability_name(&Capability::StellarSdkDependency),
            "stellar-sdk-dependency"
        );
        assert_eq!(capability_name(&Capability::WasmArtifact), "wasm-artifact");
        assert_eq!(
            capability_name(&Capability::RawLedgerAccess),
            "raw-ledger-access"
        );
    }
}
