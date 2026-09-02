//! Typed representation of `.stellar-canary.toml`.

use serde::{Deserialize, Serialize};

use canary_core::ProjectType;

/// The only configuration schema version this build understands.
///
/// Bump this, and add explicit migration/rejection logic, before changing
/// the shape of [`ConfigFile`] in a way that would silently misread an
/// older file.
pub const SUPPORTED_CONFIG_VERSION: u32 = 1;

/// The default target protocol used when a project does not pin one.
pub const DEFAULT_PROTOCOL: u32 = 28;

/// The parsed, typed contents of `.stellar-canary.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigFile {
    pub version: u32,
    pub protocol: u32,
    #[serde(default)]
    pub project: ProjectSection,
    #[serde(default)]
    pub tests: TestsSection,
    #[serde(default)]
    pub policy: PolicySection,
}

impl Default for ConfigFile {
    fn default() -> Self {
        ConfigFile {
            version: SUPPORTED_CONFIG_VERSION,
            protocol: DEFAULT_PROTOCOL,
            project: ProjectSection::default(),
            tests: TestsSection::default(),
            policy: PolicySection::default(),
        }
    }
}

/// `[project]` — how to classify the project under test.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectSection {
    #[serde(rename = "type", default)]
    pub project_type: ProjectTypeSetting,
}

/// Either `"auto"` (run detection) or an explicit, user-declared project
/// type that overrides detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectTypeSetting {
    #[default]
    Auto,
    Explicit(ProjectType),
}

impl ProjectTypeSetting {
    fn as_str(&self) -> &'static str {
        match self {
            ProjectTypeSetting::Auto => "auto",
            ProjectTypeSetting::Explicit(ProjectType::Soroban) => "soroban",
            ProjectTypeSetting::Explicit(ProjectType::RpcConsumer) => "rpc-consumer",
            ProjectTypeSetting::Explicit(ProjectType::StellarSdk) => "stellar-sdk",
            ProjectTypeSetting::Explicit(ProjectType::GenericStellar) => "generic-stellar",
            ProjectTypeSetting::Explicit(ProjectType::Unknown) => "unknown",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        Some(match value {
            "auto" => ProjectTypeSetting::Auto,
            "soroban" => ProjectTypeSetting::Explicit(ProjectType::Soroban),
            "rpc-consumer" => ProjectTypeSetting::Explicit(ProjectType::RpcConsumer),
            "stellar-sdk" => ProjectTypeSetting::Explicit(ProjectType::StellarSdk),
            "generic-stellar" => ProjectTypeSetting::Explicit(ProjectType::GenericStellar),
            "unknown" => ProjectTypeSetting::Explicit(ProjectType::Unknown),
            _ => return None,
        })
    }
}

impl Serialize for ProjectTypeSetting {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProjectTypeSetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        ProjectTypeSetting::from_str(&raw).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid project type {raw:?}: expected one of \"auto\", \"soroban\", \
                 \"rpc-consumer\", \"stellar-sdk\", \"generic-stellar\", \"unknown\""
            ))
        })
    }
}

/// `[tests]` — which surfaces are enabled for this project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestsSection {
    #[serde(default = "default_true")]
    pub xdr: bool,
    #[serde(default = "default_true")]
    pub rpc: bool,
    #[serde(default = "default_true")]
    pub soroban: bool,
}

fn default_true() -> bool {
    true
}

impl Default for TestsSection {
    fn default() -> Self {
        TestsSection {
            xdr: true,
            rpc: true,
            soroban: true,
        }
    }
}

/// `[policy]` — how results are turned into a pass/warn/fail decision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicySection {
    #[serde(default)]
    pub warnings_are_failures: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_the_documented_mvp_example() {
        let config = ConfigFile::default();
        assert_eq!(config.version, 1);
        assert_eq!(config.protocol, 28);
        assert_eq!(config.project.project_type, ProjectTypeSetting::Auto);
        assert!(config.tests.xdr && config.tests.rpc && config.tests.soroban);
        assert!(!config.policy.warnings_are_failures);
    }

    #[test]
    fn project_type_setting_round_trips_through_strings() {
        for setting in [
            ProjectTypeSetting::Auto,
            ProjectTypeSetting::Explicit(ProjectType::Soroban),
            ProjectTypeSetting::Explicit(ProjectType::RpcConsumer),
            ProjectTypeSetting::Explicit(ProjectType::StellarSdk),
            ProjectTypeSetting::Explicit(ProjectType::GenericStellar),
        ] {
            let json = serde_json::to_string(&setting).unwrap();
            let parsed: ProjectTypeSetting = serde_json::from_str(&json).unwrap();
            assert_eq!(setting, parsed);
        }
    }

    #[test]
    fn unknown_project_type_string_is_rejected() {
        let result: Result<ProjectTypeSetting, _> = serde_json::from_str("\"not-a-type\"");
        assert!(result.is_err());
    }
}
