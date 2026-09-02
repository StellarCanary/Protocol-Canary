//! Machine-readable JSON output.
//!
//! The JSON shape is defined explicitly here rather than derived directly
//! from `canary_core::CompatibilityResult`, so that a change to that
//! internal type can never silently change this schema — only an
//! intentional edit to this file can, and that edit must bump
//! [`SCHEMA_VERSION`].

use serde::{Deserialize, Serialize};

use canary_core::{
    CompatibilityResult, GitContext, NetworkName, PolicyDecision, ProjectType, ProtocolVersion,
    Status, Surface,
};

use crate::{NetworkSummary, ProjectSummary, ReportInput, SkipSummary};

/// The current JSON report schema version. Bump this, and keep the old
/// shape available if practical, whenever a change here would not be
/// backward compatible for an existing consumer. Purely additive fields
/// (like `git`) do not require a bump.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct JsonReport {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "toolVersion")]
    tool_version: String,
    #[serde(rename = "targetProtocol")]
    target_protocol: u32,
    project: JsonProject,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    network: Option<JsonNetwork>,
    status: String,
    results: Vec<JsonResult>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    skipped: Vec<JsonSkip>,
    #[serde(default)]
    git: JsonGit,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonProject {
    name: String,
    #[serde(rename = "type")]
    project_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonNetwork {
    name: String,
    #[serde(rename = "observedProtocol", skip_serializing_if = "Option::is_none")]
    observed_protocol: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonResult {
    #[serde(rename = "testId")]
    test_id: String,
    protocol: u32,
    surface: String,
    status: String,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    details: Option<String>,
    #[serde(rename = "durationMs")]
    duration_ms: u64,
    #[serde(rename = "fixtureId", skip_serializing_if = "Option::is_none", default)]
    fixture_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonSkip {
    #[serde(rename = "fixtureId")]
    fixture_id: String,
    surface: String,
    reason: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct JsonGit {
    #[serde(default)]
    commit: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(rename = "isDirty", default)]
    is_dirty: Option<bool>,
}

impl From<&ReportInput> for JsonReport {
    fn from(input: &ReportInput) -> Self {
        JsonReport {
            schema_version: SCHEMA_VERSION,
            tool_version: input.tool_version.clone(),
            target_protocol: input.target_protocol.0,
            project: JsonProject {
                name: input.project.name.clone(),
                project_type: input.project.project_type.to_string(),
            },
            network: input.network.as_ref().map(|n| JsonNetwork {
                name: n.name.to_string(),
                observed_protocol: n.observed_protocol.map(|p| p.0),
            }),
            status: input.overall_status().as_str().to_string(),
            results: input
                .results
                .iter()
                .map(|r| JsonResult {
                    test_id: r.test_id.clone(),
                    protocol: r.protocol.0,
                    surface: r.surface.to_string(),
                    status: r.status.to_string(),
                    summary: r.summary.clone(),
                    details: r.details.clone(),
                    duration_ms: r.duration_ms,
                    fixture_id: r.fixture_id.clone(),
                })
                .collect(),
            skipped: input
                .skipped
                .iter()
                .map(|s| JsonSkip {
                    fixture_id: s.fixture_id.clone(),
                    surface: s.surface.to_string(),
                    reason: s.reason.clone(),
                })
                .collect(),
            git: JsonGit {
                commit: input.git.commit.clone(),
                branch: input.git.branch.clone(),
                is_dirty: input.git.is_dirty,
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JsonReportError {
    #[error("failed to parse JSON report: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("unrecognized surface {0:?} in JSON report")]
    UnknownSurface(String),
    #[error("unrecognized status {0:?} in JSON report")]
    UnknownStatus(String),
}

fn parse_surface(value: &str) -> Result<Surface, JsonReportError> {
    match value {
        "xdr" => Ok(Surface::Xdr),
        "rpc" => Ok(Surface::Rpc),
        "soroban" => Ok(Surface::Soroban),
        other => Err(JsonReportError::UnknownSurface(other.to_string())),
    }
}

fn parse_status(value: &str) -> Result<Status, JsonReportError> {
    match value {
        "pass" => Ok(Status::Pass),
        "warning" => Ok(Status::Warning),
        "fail" => Ok(Status::Fail),
        "skipped" => Ok(Status::Skipped),
        "error" => Ok(Status::Error),
        other => Err(JsonReportError::UnknownStatus(other.to_string())),
    }
}

fn parse_network_name(value: &str) -> NetworkName {
    match value {
        "testnet" => NetworkName::Testnet,
        "mainnet" => NetworkName::Mainnet,
        "futurenet" => NetworkName::Futurenet,
        other => NetworkName::Custom(other.to_string()),
    }
}

fn parse_project_type(value: &str) -> ProjectType {
    match value {
        "soroban" => ProjectType::Soroban,
        "rpc-consumer" => ProjectType::RpcConsumer,
        "stellar-sdk" => ProjectType::StellarSdk,
        "generic-stellar" => ProjectType::GenericStellar,
        _ => ProjectType::Unknown,
    }
}

impl TryFrom<JsonReport> for ReportInput {
    type Error = JsonReportError;

    fn try_from(report: JsonReport) -> Result<Self, Self::Error> {
        let results = report
            .results
            .into_iter()
            .map(|r| {
                Ok(CompatibilityResult {
                    test_id: r.test_id,
                    protocol: ProtocolVersion(r.protocol),
                    surface: parse_surface(&r.surface)?,
                    status: parse_status(&r.status)?,
                    summary: r.summary,
                    details: r.details,
                    duration_ms: r.duration_ms,
                    fixture_id: r.fixture_id,
                })
            })
            .collect::<Result<Vec<_>, JsonReportError>>()?;

        let skipped = report
            .skipped
            .into_iter()
            .map(|s| {
                Ok(SkipSummary {
                    fixture_id: s.fixture_id,
                    surface: parse_surface(&s.surface)?,
                    reason: s.reason,
                })
            })
            .collect::<Result<Vec<_>, JsonReportError>>()?;

        // The stored "status" is the overall outcome (with an execution
        // error already taking precedence, see ReportInput::overall_status),
        // not the raw policy decision. Re-deriving a decision from it is
        // lossy only for the "error" case, and that case is recovered
        // automatically: `results` still carries any Status::Error entries,
        // so `overall_status()` reports Error regardless of this fallback.
        let decision = match report.status.as_str() {
            "pass" => PolicyDecision::Pass,
            "warning" => PolicyDecision::Warning,
            _ => PolicyDecision::Fail,
        };

        Ok(ReportInput {
            tool_version: report.tool_version,
            target_protocol: ProtocolVersion(report.target_protocol),
            project: ProjectSummary {
                name: report.project.name,
                project_type: parse_project_type(&report.project.project_type),
            },
            network: report.network.map(|n| NetworkSummary {
                name: parse_network_name(&n.name),
                observed_protocol: n.observed_protocol.map(ProtocolVersion),
            }),
            results,
            skipped,
            decision,
            git: GitContext {
                commit: report.git.commit,
                branch: report.git.branch,
                is_dirty: report.git.is_dirty,
            },
            verbose: false,
        })
    }
}

/// Renders a [`ReportInput`] as versioned, deterministic JSON.
pub struct JsonReporter;

impl JsonReporter {
    pub fn render(input: &ReportInput) -> String {
        let report = JsonReport::from(input);
        serde_json::to_string_pretty(&report)
            .unwrap_or_else(|e| format!("{{\"error\": \"failed to serialize report: {e}\"}}"))
    }

    /// Parses a previously rendered JSON report back into a [`ReportInput`]
    /// so it can be re-rendered in another format without re-running
    /// anything.
    pub fn parse(json_text: &str) -> Result<ReportInput, JsonReportError> {
        let report: JsonReport = serde_json::from_str(json_text)?;
        report.try_into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NetworkSummary, ProjectSummary, SkipSummary};
    use canary_core::{
        CompatibilityResult, GitContext, NetworkName, PolicyDecision, ProjectType, ProtocolVersion,
        Status, Surface,
    };

    fn input() -> ReportInput {
        ReportInput {
            tool_version: "0.1.0".into(),
            target_protocol: ProtocolVersion(28),
            project: ProjectSummary {
                name: "example".into(),
                project_type: ProjectType::Soroban,
            },
            network: Some(NetworkSummary {
                name: NetworkName::Testnet,
                observed_protocol: Some(ProtocolVersion(28)),
            }),
            results: vec![CompatibilityResult {
                test_id: "p28-xdr-1".into(),
                protocol: ProtocolVersion(28),
                surface: Surface::Xdr,
                status: Status::Pass,
                summary: "decoded successfully".into(),
                details: None,
                duration_ms: 3,
                fixture_id: Some("p28-xdr-1".into()),
            }],
            skipped: vec![SkipSummary {
                fixture_id: "p28-soroban-1".into(),
                surface: Surface::Soroban,
                reason: "requires a capability not declared by this project".into(),
            }],
            decision: PolicyDecision::Pass,
            git: GitContext::default(),
            verbose: false,
        }
    }

    #[test]
    fn matches_the_documented_top_level_shape() {
        let json_text = JsonReporter::render(&input());
        let value: serde_json::Value = serde_json::from_str(&json_text).unwrap();

        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["toolVersion"], "0.1.0");
        assert_eq!(value["targetProtocol"], 28);
        assert_eq!(value["project"]["name"], "example");
        assert_eq!(value["project"]["type"], "soroban");
        assert_eq!(value["network"]["name"], "testnet");
        assert_eq!(value["network"]["observedProtocol"], 28);
        assert_eq!(value["status"], "pass");
        assert!(value["results"].is_array());
        assert_eq!(value["results"][0]["testId"], "p28-xdr-1");
        assert_eq!(value["skipped"][0]["fixtureId"], "p28-soroban-1");
    }

    #[test]
    fn an_execution_error_is_reflected_in_the_status_field() {
        let mut input = input();
        input.results[0].status = Status::Error;
        let json_text = JsonReporter::render(&input);
        let value: serde_json::Value = serde_json::from_str(&json_text).unwrap();
        assert_eq!(value["status"], "error");
    }

    #[test]
    fn omits_the_network_field_entirely_for_an_offline_run() {
        let mut input = input();
        input.network = None;
        let json_text = JsonReporter::render(&input);
        let value: serde_json::Value = serde_json::from_str(&json_text).unwrap();
        assert!(value.get("network").is_none());
    }

    #[test]
    fn output_is_deterministic_for_the_same_input() {
        let a = JsonReporter::render(&input());
        let b = JsonReporter::render(&input());
        assert_eq!(a, b);
    }

    #[test]
    fn parses_its_own_rendered_output_back_into_an_equivalent_report() {
        let original = input();
        let json_text = JsonReporter::render(&original);
        let parsed = JsonReporter::parse(&json_text).expect("parses");

        assert_eq!(parsed.tool_version, original.tool_version);
        assert_eq!(parsed.target_protocol, original.target_protocol);
        assert_eq!(parsed.project, original.project);
        assert_eq!(parsed.network, original.network);
        assert_eq!(parsed.results, original.results);
        assert_eq!(parsed.skipped, original.skipped);
        assert_eq!(parsed.overall_status(), original.overall_status());
    }

    #[test]
    fn parsing_preserves_an_error_outcome_even_though_decision_is_approximated() {
        let mut original = input();
        original.results[0].status = Status::Error;
        let json_text = JsonReporter::render(&original);
        let parsed = JsonReporter::parse(&json_text).expect("parses");
        assert_eq!(parsed.overall_status(), crate::ReportStatus::Error);
    }

    #[test]
    fn rejects_malformed_json() {
        let err = JsonReporter::parse("not json").unwrap_err();
        assert!(matches!(err, JsonReportError::Parse(_)));
    }
}
