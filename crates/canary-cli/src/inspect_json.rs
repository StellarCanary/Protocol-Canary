//! The machine-readable document `stellar-canary inspect --format json`
//! prints (CA-03, issue #453), shared by the parity tests that assert
//! the plan equals what `check` executes (issue #485).
//!
//! The shape is written out explicitly, like `canary_report::json`, so a
//! change to an internal type can never silently change the document.
//! Conforms to CA-03, CF-07, and CA-04.
//!
//! [`InspectDocument`] is the whole response: the project and its detection
//! evidence, the effective configuration and target, the fixture count, the
//! offline plan `check` would build, the workspace and the project
//! fingerprint. The types below are the nested values of that one document
//! and are consumed together; none of the top-level fields is optional.

use serde::Serialize;

use canary_core::Capability;

/// The current inspect document version (`inspectVersion`).
pub const INSPECT_VERSION: u32 = 1;

/// The root object of the CA-03 inspection response.
///
/// All sections are always present. [`InspectProject`] and
/// [`InspectDetection`] describe the detected project; [`InspectConfig`] and
/// [`InspectTarget`] carry the effective settings the plan was built from;
/// [`InspectFixtures`] and [`InspectPlan`] preview what `check` would do.
/// [`InspectWorkspace`] and [`InspectFingerprint`] describe the project shape.
/// [`render`] writes the fields in declaration order with lower camelCase
/// names, two-space indentation, LF line endings and a final newline.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectDocument {
    /// The document shape version, [`INSPECT_VERSION`].
    pub inspect_version: u32,
    /// The CLI's own version, independent of the document version.
    pub tool_version: String,
    /// Always `true`: `inspect` never opens a network connection. A
    /// consumer can assert it.
    pub offline: bool,
    /// The effective project type, detected capabilities and their evidence.
    pub project: InspectProject,
    /// Whether a configuration file was read, and the effective settings.
    pub config: InspectConfig,
    /// The protocol the plan was built for and where it came from.
    pub target: InspectTarget,
    /// The fixture directory's status and how many fixtures were loaded.
    pub fixtures: InspectFixtures,
    /// The offline preview of the work `check` would plan.
    pub plan: InspectPlan,
    /// Workspace members detected in the project, also under `project.workspace`.
    pub workspace: InspectWorkspace,
    /// The CA-04 fingerprint of the detected project shape.
    pub fingerprint: InspectFingerprint,
}

/// The effective project identity, capabilities and detection result.
///
/// `type` is the project type `check` would use; `detectedType` is kept
/// alongside it so a configured override remains visible. `detection` holds
/// the evidence behind both.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectProject {
    /// The selected root relative to the directory the command was
    /// started in. Never absolute.
    pub root: String,
    /// How the root was selected: `flag` or `current-directory`.
    pub root_selected_by: String,
    /// The effective project type, after any configuration override.
    #[serde(rename = "type")]
    pub project_type: String,
    /// `detected` or `config`.
    pub type_source: String,
    /// What detection produced before any configuration override.
    pub detected_type: String,
    /// Sorted, lowercase kebab.
    pub capabilities: Vec<String>,
    /// The workspace members reported by detection.
    pub workspace: InspectWorkspace,
    /// The evidence collected by detection.
    pub detection: InspectDetection,
}

/// Workspace members reported by project detection.
///
/// The same snapshot is serialized both here under `project.workspace` and
/// as the top-level `workspace`. `members` is always an array, empty when no
/// members were found.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectWorkspace {
    /// Root-relative paths of the workspace members.
    pub members: Vec<String>,
}

/// The CA-04 fingerprint of the detected project shape.
///
/// All four fields are always present. `inputs` is the number of files
/// hashed into `digest`, not a list of those files.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectFingerprint {
    /// The fingerprint format version.
    pub fingerprint_version: u32,
    /// The revision of the detector that produced the fingerprint.
    pub detector: u64,
    /// The `sha256:` digest of the fingerprint inputs.
    pub digest: String,
    /// Number of inspected files contributing to `digest`.
    pub inputs: usize,
}

/// The evidence collected while detecting the project (CF-07).
///
/// All fields are always present. Paths are relative to the selected project
/// root: `root` names the scope of detection and `inspected` lists the
/// manifests and other files that were read.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectDetection {
    /// The detection evidence format version.
    pub evidence_version: u32,
    /// The root detection was run against, relative to the selected root.
    pub root: String,
    /// One entry per capability with the evidence that produced it.
    pub capabilities: Vec<InspectCapabilityEvidence>,
    /// Root-relative paths of the files detection inspected.
    pub inspected: Vec<String>,
    /// Non-fatal problems or notices raised while inspecting the project.
    pub diagnostics: Vec<InspectDiagnostic>,
    /// Evidence for the effective project type.
    pub project_type: InspectProjectTypeEvidence,
}

/// One detected capability and the evidence that produced it.
///
/// This is an entry in [`InspectDetection::capabilities`]; `evidence` holds
/// the observations detection recorded for that capability.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectCapabilityEvidence {
    /// The capability's lowercase kebab name.
    pub capability: String,
    /// The observed evidence items for that capability.
    pub evidence: Vec<InspectEvidenceItem>,
}

/// One observation that contributed to a detected capability.
///
/// `manifest` and `ecosystem` are always present. The remaining fields are
/// omitted, never emitted as `null`, when the observation has no such value:
/// dependency evidence carries `dependency` and `section`, while artifact
/// evidence is described by `sourceFile`, `matchedText` and `reason`.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectEvidenceItem {
    /// Root-relative path of the manifest or artifact examined.
    pub manifest: String,
    /// The ecosystem that supplied the evidence, for example `cargo`.
    pub ecosystem: String,
    /// The matching dependency name, when the evidence is dependency-based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency: Option<String>,
    /// The manifest section (for example `dependencies`), when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Root-relative path of the file the evidence came from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    /// The dependency, string or artifact identifier that matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_text: Option<String>,
    /// Why this observation produces the capability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A non-fatal diagnostic raised while inspecting project files.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectDiagnostic {
    /// The detector's stable diagnostic code, for example `manifest-unparseable`.
    pub code: String,
    /// Human-readable detail about what happened.
    pub detail: String,
}

/// Evidence for the effective project type in [`InspectDetection::project_type`].
///
/// `detectedValue` is omitted, not `null`, unless an explicit configured type
/// differs from what detection produced.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectProjectTypeEvidence {
    /// The effective project type, matching `project.type`.
    pub value: String,
    /// Where that type came from: `config` or `detected`.
    pub source: String,
    /// The type detection produced when configuration overrode it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_value: Option<String>,
}

/// Whether a configuration file was read, and the effective settings used.
///
/// `found` and `path` describe the file itself. The other fields hold the
/// effective values after defaults, so they are present even when no file was
/// read. `path` is serialized as `null`, not omitted, when no file was read
/// or when an explicit `--config` points outside the root.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectConfig {
    /// Whether a configuration file was read.
    pub found: bool,
    /// The configuration file's path relative to the root, with `/`. `null`
    /// when no file was read, or when an explicit `--config` points
    /// outside the root.
    pub path: Option<String>,
    /// The configuration schema version.
    pub version: u32,
    /// The configured protocol, before any `--protocol` flag is applied.
    pub protocol: u32,
    /// `auto` or an explicit type name.
    pub project_type: String,
    /// Enabled compatibility surfaces from `[tests]`.
    pub tests: InspectTests,
    /// Report policy settings from `[policy]`.
    pub policy: InspectPolicy,
}

/// The `[tests]` surface switches from the effective configuration.
///
/// All three booleans are always present, using the configuration file's
/// values or the built-in defaults when no file was read.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectTests {
    /// Whether XDR checks are enabled.
    pub xdr: bool,
    /// Whether RPC checks are enabled.
    pub rpc: bool,
    /// Whether Soroban checks are enabled.
    pub soroban: bool,
}

/// The `[policy]` settings from the effective configuration.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectPolicy {
    /// Whether warnings are treated as failures, serialized as
    /// `warningsAreFailures`.
    pub warnings_are_failures: bool,
}

/// The protocol the plan was built for, and where it came from.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectTarget {
    /// The target protocol used to select applicable fixtures.
    pub protocol: u32,
    /// `flag`, `config` or `default`.
    pub source: String,
}

/// The fixture directory's status and how many fixtures were loaded.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectFixtures {
    /// Whether the resolved fixture directory exists.
    pub directory_found: bool,
    /// Number of fixtures loaded from the directory, across all protocols.
    pub loaded: usize,
}

/// The offline preview of the plan `check` would execute for this project.
///
/// Both lists are always present, possibly empty. `wouldRun` follows `check`'s
/// execution order (XDR, then RPC, then Soroban, each in planned order);
/// `skipped` follows the planner's order. `checkWouldRefuse` is true exactly
/// when `wouldRun` is empty. `refusal` is serialized as `null` when there is
/// no explanation, rather than being omitted.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectPlan {
    /// `{id, surface}` for every fixture `check` would execute, in the
    /// order `check` executes them.
    pub would_run: Vec<PlannedFixture>,
    /// `{id, surface, code, reason}` for every fixture left out, in the
    /// order `check` records them.
    pub skipped: Vec<PlannedSkip>,
    /// Whether `check` would refuse an empty run unless `--allow-empty` is given.
    pub check_would_refuse: bool,
    /// The explanation `check` would print for an empty run, or `null`.
    pub refusal: Option<String>,
}

/// A fixture `check` would execute, identified by its id and surface.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFixture {
    /// The fixture id as loaded from its file.
    pub id: String,
    /// The surface that would execute it: `xdr`, `rpc` or `soroban`.
    pub surface: String,
}

/// A loaded fixture the planner left out of the check run.
///
/// The fields mirror the same skip `check` reports: `code` is the stable
/// CF-01 code from `SkipCause::code()`, and `reason` is the descriptive text.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSkip {
    /// The id of the skipped fixture.
    pub id: String,
    /// The surface the fixture targets.
    pub surface: String,
    /// The CF-01 skip code, for example `UNSUPPORTED_PROTOCOL`.
    pub code: String,
    /// The human-readable skip explanation `check` also records.
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
                            source_file: Some("contracts/token/Cargo.toml".to_string()),
                            matched_text: Some("soroban-sdk".to_string()),
                            reason: Some("cargo dependencies declares soroban-sdk".to_string()),
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
                    code: "UNSUPPORTED_PROTOCOL".to_string(),
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
        assert_eq!(value["plan"]["skipped"][0]["code"], "UNSUPPORTED_PROTOCOL");
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
