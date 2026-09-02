//! The compatibility domain model shared by every crate in the workspace.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A Stellar protocol version number (e.g. `28`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtocolVersion(pub u32);

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for ProtocolVersion {
    fn from(value: u32) -> Self {
        ProtocolVersion(value)
    }
}

/// A compatibility surface a project can be checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Xdr,
    Rpc,
    Soroban,
}

impl fmt::Display for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Surface::Xdr => "xdr",
            Surface::Rpc => "rpc",
            Surface::Soroban => "soroban",
        };
        f.write_str(name)
    }
}

/// The outcome of running a single [`crate::engine::CompatibilityTest`].
///
/// `Fail` means the compatibility assertion ran and failed; `Error` means
/// the test could not be executed correctly (e.g. a network timeout). Do
/// not conflate the two: a `Fail` is real, reproducible evidence about
/// compatibility, while an `Error` says nothing about compatibility at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warning,
    Fail,
    Skipped,
    Error,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Status::Pass => "pass",
            Status::Warning => "warning",
            Status::Fail => "fail",
            Status::Skipped => "skipped",
            Status::Error => "error",
        };
        f.write_str(name)
    }
}

/// The normalized result of a single compatibility test, produced by every
/// surface runner and consumed by every reporter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityResult {
    pub test_id: String,
    pub protocol: ProtocolVersion,
    pub surface: Surface,
    pub status: Status,
    pub summary: String,
    pub details: Option<String>,
    pub duration_ms: u64,
    pub fixture_id: Option<String>,
}

impl CompatibilityResult {
    pub fn is_required_failure(&self) -> bool {
        matches!(self.status, Status::Fail | Status::Error)
    }
}

/// How a project was classified by [project detection](crate::ProjectContext).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectType {
    Soroban,
    RpcConsumer,
    StellarSdk,
    GenericStellar,
    Unknown,
}

impl fmt::Display for ProjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ProjectType::Soroban => "soroban",
            ProjectType::RpcConsumer => "rpc-consumer",
            ProjectType::StellarSdk => "stellar-sdk",
            ProjectType::GenericStellar => "generic-stellar",
            ProjectType::Unknown => "unknown",
        };
        f.write_str(name)
    }
}

/// A declared or detected capability of the project under test.
///
/// Used by the [planner](crate::planner::CompatibilityPlanner) to decide
/// whether a fixture that requires a capability is applicable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    SorobanContract,
    RpcClient,
    StellarSdkDependency,
    WasmArtifact,
    RawLedgerAccess,
}

/// Everything the pipeline knows about the project being checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectContext {
    pub root: std::path::PathBuf,
    pub name: String,
    pub project_type: ProjectType,
    pub capabilities: Vec<Capability>,
}

impl ProjectContext {
    pub fn has_capability(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }
}

/// Which Stellar network a live check targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkName {
    Testnet,
    Mainnet,
    Futurenet,
    Custom(String),
}

impl fmt::Display for NetworkName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkName::Testnet => f.write_str("testnet"),
            NetworkName::Mainnet => f.write_str("mainnet"),
            NetworkName::Futurenet => f.write_str("futurenet"),
            NetworkName::Custom(name) => f.write_str(name),
        }
    }
}

/// The network a live (RPC/Soroban) check runs against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkContext {
    pub name: NetworkName,
    pub rpc_url: String,
    pub passphrase: String,
    pub observed_protocol: Option<ProtocolVersion>,
}

/// Git repository metadata for a run, when the project is a Git checkout.
///
/// Fields are `None` rather than an error when Git metadata is genuinely
/// unavailable (not a repository, no commits yet, etc.) — normal CLI usage
/// outside of Git must not fail a compatibility run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitContext {
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub is_dirty: Option<bool>,
}

/// User-controlled knobs for a single run, populated from CLI flags and
/// configuration file defaults (CLI flags win).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOptions {
    pub verbose: bool,
    pub quiet: bool,
    pub max_concurrency: u32,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            verbose: false,
            quiet: false,
            max_concurrency: 4,
        }
    }
}

/// Metadata describing one fixture, independent of its surface-specific
/// input/expectation payload (which lives in the surface crate that
/// interprets it, e.g. `canary-xdr::XdrFixture`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureMetadata {
    pub id: String,
    pub protocol: ProtocolVersion,
    pub surface: Surface,
    pub category: String,
    pub description: String,
    pub source_reference: Option<String>,
    pub required_capabilities: Vec<Capability>,
}

/// An in-memory collection of loaded, validated fixture metadata.
///
/// This holds metadata only; the surface-specific fixture body is loaded
/// separately by the runner that needs it, keyed by [`FixtureMetadata::id`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureStore {
    fixtures: Vec<FixtureMetadata>,
}

impl FixtureStore {
    pub fn new(fixtures: Vec<FixtureMetadata>) -> Self {
        FixtureStore { fixtures }
    }

    pub fn all(&self) -> &[FixtureMetadata] {
        &self.fixtures
    }

    pub fn len(&self) -> usize {
        self.fixtures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fixtures.is_empty()
    }

    pub fn by_id(&self, id: &str) -> Option<&FixtureMetadata> {
        self.fixtures.iter().find(|f| f.id == id)
    }

    pub fn for_surface(&self, surface: Surface) -> impl Iterator<Item = &FixtureMetadata> {
        self.fixtures.iter().filter(move |f| f.surface == surface)
    }

    pub fn for_protocol(
        &self,
        protocol: ProtocolVersion,
    ) -> impl Iterator<Item = &FixtureMetadata> {
        self.fixtures.iter().filter(move |f| f.protocol == protocol)
    }
}

/// The set of fixtures known for a given protocol version.
///
/// This is the seam that lets a new protocol version be added as data
/// rather than as new branches scattered through the engine: adding
/// `protocol-29` means constructing a new `ProtocolPack`, not editing the
/// `protocol-28` one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolPack {
    pub version: ProtocolVersion,
    pub fixtures: Vec<FixtureMetadata>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_displays_as_its_number() {
        assert_eq!(ProtocolVersion(28).to_string(), "28");
    }

    #[test]
    fn protocol_versions_order_numerically() {
        assert!(ProtocolVersion(27) < ProtocolVersion(28));
    }

    #[test]
    fn fixture_store_filters_by_surface_and_protocol() {
        let store = FixtureStore::new(vec![
            FixtureMetadata {
                id: "p28-xdr-cap83-001".into(),
                protocol: ProtocolVersion(28),
                surface: Surface::Xdr,
                category: "cap-83".into(),
                description: "StellarValue roundtrip".into(),
                source_reference: Some("CAP-0083".into()),
                required_capabilities: vec![],
            },
            FixtureMetadata {
                id: "p27-rpc-001".into(),
                protocol: ProtocolVersion(27),
                surface: Surface::Rpc,
                category: "network".into(),
                description: "network identity".into(),
                source_reference: None,
                required_capabilities: vec![],
            },
        ]);

        assert_eq!(store.len(), 2);
        assert_eq!(store.for_surface(Surface::Xdr).count(), 1);
        assert_eq!(store.for_protocol(ProtocolVersion(28)).count(), 1);
        assert!(store.by_id("p28-xdr-cap83-001").is_some());
        assert!(store.by_id("missing").is_none());
    }

    #[test]
    fn compatibility_result_distinguishes_fail_from_error() {
        let fail = CompatibilityResult {
            test_id: "t1".into(),
            protocol: ProtocolVersion(28),
            surface: Surface::Xdr,
            status: Status::Fail,
            summary: "decode mismatch".into(),
            details: None,
            duration_ms: 1,
            fixture_id: Some("p28-xdr-cap83-001".into()),
        };
        let error = CompatibilityResult {
            status: Status::Error,
            ..fail.clone()
        };
        assert!(fail.is_required_failure());
        assert!(error.is_required_failure());
        assert_ne!(fail.status, error.status);
    }

    #[test]
    fn project_context_reports_declared_capabilities() {
        let ctx = ProjectContext {
            root: std::path::PathBuf::from("."),
            name: "example".into(),
            project_type: ProjectType::Soroban,
            capabilities: vec![Capability::SorobanContract],
        };
        assert!(ctx.has_capability(&Capability::SorobanContract));
        assert!(!ctx.has_capability(&Capability::RawLedgerAccess));
    }
}
