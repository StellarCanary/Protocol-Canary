//! Machine-readable JSON output.
//!
//! The JSON shape is defined explicitly here rather than derived directly
//! from `canary_core::CompatibilityResult`, so that a change to that
//! internal type can never silently change this schema — only an
//! intentional edit to this file can, and that edit must bump
//! [`SCHEMA_VERSION`].

use serde::Serialize;

use crate::ReportInput;

/// The current JSON report schema version. Bump this, and keep the old
/// shape available if practical, whenever a change here would not be
/// backward compatible for an existing consumer.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
struct JsonReport {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "toolVersion")]
    tool_version: String,
    #[serde(rename = "targetProtocol")]
    target_protocol: u32,
    project: JsonProject,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<JsonNetwork>,
    status: String,
    results: Vec<JsonResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<JsonSkip>,
}

#[derive(Debug, Serialize)]
struct JsonProject {
    name: String,
    #[serde(rename = "type")]
    project_type: String,
}

#[derive(Debug, Serialize)]
struct JsonNetwork {
    name: String,
    #[serde(rename = "observedProtocol", skip_serializing_if = "Option::is_none")]
    observed_protocol: Option<u32>,
}

#[derive(Debug, Serialize)]
struct JsonResult {
    #[serde(rename = "testId")]
    test_id: String,
    protocol: u32,
    surface: String,
    status: String,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(rename = "durationMs")]
    duration_ms: u64,
    #[serde(rename = "fixtureId", skip_serializing_if = "Option::is_none")]
    fixture_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct JsonSkip {
    #[serde(rename = "fixtureId")]
    fixture_id: String,
    surface: String,
    reason: String,
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
        }
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
}
