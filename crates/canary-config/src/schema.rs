//! Typed representation of `.stellar-canary.toml`.

use serde::{Deserialize, Serialize};

use canary_core::ProjectType;

/// The only configuration schema version this build understands.
///
/// Bump this, and add explicit migration/rejection logic, before changing
/// the shape of [`ConfigFile`] in a way that would silently misread an
/// older file.
///
/// The current rejection logic lives in `crate::loader::validate`, which
/// returns [`ConfigError::UnsupportedVersion`](crate::loader::ConfigError::UnsupportedVersion)
/// for any other version. A version bump needs to extend that check.
pub const SUPPORTED_CONFIG_VERSION: u32 = 1;

/// The default target protocol used when a project does not pin one.
pub const DEFAULT_PROTOCOL: u32 = 28;

/// The parsed, typed contents of `.stellar-canary.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct PolicySection {
    #[serde(default)]
    pub warnings_are_failures: bool,
}

/// The only `.stellar-canary.lock` version this build understands
/// (CF-03 section 3, rule 1).
pub const SUPPORTED_LOCK_VERSION: u32 = 1;

/// The only fixture registry version this build can verify a pack against
/// (CF-03 section 3, rule 5; the registry format is defined by CF-02).
pub const SUPPORTED_REGISTRY_VERSION: u32 = 1;

/// The largest lockfile that is read, in bytes: 64 KiB (CF-03 section 2).
pub const MAX_LOCK_FILE_BYTES: u64 = 64 * 1024;

/// The parsed, typed contents of `.stellar-canary.lock` (CF-03).
///
/// A lock pins what a run is checked against: the fixture pack and the
/// protocol. Unknown keys and tables are rejected rather than ignored,
/// because an ignored key could silently drop a pin.
///
/// This type has no `Serialize` implementation on purpose. The file format
/// is defined byte for byte (key order, quoting, newlines), so writing goes
/// through [`crate::loader::serialize_lock`] and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockFile {
    /// Always [`SUPPORTED_LOCK_VERSION`] in a value that passed validation.
    #[serde(rename = "lockVersion")]
    pub lock_version: u32,
    /// `[tool]`: the engine version the lock was written with. Optional.
    #[serde(default)]
    pub tool: Option<LockTool>,
    /// `[fixtures]`: the pinned fixture pack. Required.
    pub fixtures: LockFixtures,
}

/// `[tool]` in a lockfile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockTool {
    /// `MAJOR.MINOR.PATCH`, with no leading `v`.
    pub version: String,
}

/// `[fixtures]` in a lockfile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockFixtures {
    /// The protocol the pack was pinned for. Zero is allowed: the contract
    /// asks for a non-negative integer.
    pub protocol: u32,
    /// `sha256:` followed by 64 lowercase hex digits (CF-02).
    #[serde(rename = "packDigest")]
    pub pack_digest: String,
    /// The registry format version of the pinned pack.
    #[serde(rename = "registryVersion")]
    pub registry_version: u32,
    /// Where the pack came from. Informational.
    #[serde(default)]
    pub source: Option<LockSource>,
    /// The pack's revision. Informational only, never compared.
    #[serde(default)]
    pub revision: Option<String>,
}

/// `fixtures.source` in a lockfile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockSource {
    Directory,
    Release,
}

impl LockSource {
    /// The spelling used in the file.
    pub fn as_str(self) -> &'static str {
        match self {
            LockSource::Directory => "directory",
            LockSource::Release => "release",
        }
    }
}

impl LockFile {
    /// Checks the rules a well-typed value can still break (CF-03 section 3,
    /// rules 1, 4, 5 and 6). Returns the first violation as a message.
    ///
    /// Parsing runs this, and so does serialization, so a lock that would
    /// not be accepted on read is never written.
    pub fn validate(&self) -> Result<(), String> {
        if self.lock_version != SUPPORTED_LOCK_VERSION {
            return Err(format!(
                "lockVersion {} is not supported: this build supports version {SUPPORTED_LOCK_VERSION}",
                self.lock_version
            ));
        }
        if !is_pack_digest(&self.fixtures.pack_digest) {
            return Err(format!(
                "fixtures.packDigest {:?} is invalid: expected \"sha256:\" followed by 64 lowercase hex digits",
                self.fixtures.pack_digest
            ));
        }
        if self.fixtures.registry_version != SUPPORTED_REGISTRY_VERSION {
            return Err(format!(
                "fixtures.registryVersion {} is not supported: this build supports version {SUPPORTED_REGISTRY_VERSION}",
                self.fixtures.registry_version
            ));
        }
        if let Some(tool) = &self.tool {
            if !is_release_version(&tool.version) {
                return Err(format!(
                    "tool.version {:?} is invalid: expected MAJOR.MINOR.PATCH with no leading \"v\"",
                    tool.version
                ));
            }
        }
        Ok(())
    }
}

/// `^sha256:[0-9a-f]{64}$`, without a regular-expression dependency.
fn is_pack_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// `MAJOR.MINOR.PATCH`: three dot-separated decimal numbers, no leading
/// zeros, no `v` prefix, no pre-release or build suffix.
fn is_release_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let numeric = |part: Option<&str>| {
        part.is_some_and(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p == "0" || !p.starts_with('0'))
        })
    };
    numeric(parts.next())
        && numeric(parts.next())
        && numeric(parts.next())
        && parts.next().is_none()
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
            ProjectTypeSetting::Explicit(ProjectType::Unknown),
        ] {
            let json = serde_json::to_string(&setting).unwrap();
            let parsed: ProjectTypeSetting = serde_json::from_str(&json).unwrap();
            assert_eq!(setting, parsed);
        }
    }

    #[test]
    fn explicit_unknown_project_type_uses_its_own_wire_string() {
        // The round trip above cannot catch `Unknown` and `Auto` being
        // cross-wired to the same string, since both mappings would agree.
        let json =
            serde_json::to_string(&ProjectTypeSetting::Explicit(ProjectType::Unknown)).unwrap();
        assert_eq!(json, "\"unknown\"");
        assert_eq!(
            serde_json::from_str::<ProjectTypeSetting>("\"unknown\"").unwrap(),
            ProjectTypeSetting::Explicit(ProjectType::Unknown)
        );
    }

    #[test]
    fn unknown_project_type_string_is_rejected() {
        let result: Result<ProjectTypeSetting, _> = serde_json::from_str("\"not-a-type\"");
        assert!(result.is_err());
    }
}
