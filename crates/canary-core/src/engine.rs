//! The execution context that every surface runner is handed.

use crate::cache::CacheStore;
use crate::model::{
    FixtureStore, GitContext, NetworkContext, ProjectContext, ProtocolVersion, RunOptions,
};

/// Everything a compatibility test needs to run, explicitly. There is no
/// hidden global state: two runs constructed with equal `ExecutionContext`
/// values (modulo genuinely live network state) must behave identically.
pub struct ExecutionContext {
    /// The target Stellar protocol version this run checks compatibility
    /// against.
    ///
    /// Fixture planning selects fixtures that declare exactly this protocol,
    /// and the runner includes it in the result-cache key, so a result
    /// recorded for one protocol is never reused for another.
    pub protocol: ProtocolVersion,
    /// The project under test: its root, name, detected type and declared
    /// capabilities.
    ///
    /// Planning uses [`ProjectContext::capabilities`] to skip fixtures whose
    /// `required_capabilities` the project does not declare. The result-cache
    /// fingerprint is built from the project type, its sorted capabilities
    /// and [`ExecutionContext::git`]; `root` and `name` are deliberately
    /// excluded, so a moved or renamed checkout keeps its cached results.
    pub project: ProjectContext,
    /// The network identity and endpoint live (RPC and Soroban) checks run
    /// against.
    ///
    /// [`NetworkContext::name`], [`NetworkContext::rpc_url`] and
    /// [`NetworkContext::observed_protocol`] are part of the result-cache
    /// key, so results recorded against another network, endpoint or
    /// observed protocol are not reused. `canary_runner::execute` takes the
    /// endpoint to dial as a separate argument; the runner does not read
    /// [`NetworkContext::rpc_url`] itself, and `passphrase` is informational.
    pub network: NetworkContext,
    /// The validated fixture metadata the run was planned from.
    ///
    /// Callers build this with `canary_fixtures::validate` (or
    /// [`FixtureStore::new`]). Planning and execution receive the loaded
    /// fixtures directly, so the runner does not read this field during a
    /// run; it is neither consulted nor stored by the result cache.
    pub fixtures: FixtureStore,
    /// Git metadata for the project checkout, when it is a Git repository.
    ///
    /// [`GitContext::commit`] and [`GitContext::is_dirty`] are part of the
    /// result-cache project fingerprint, so a dirty checkout does not reuse
    /// the clean commit's cached results. Unavailable metadata is represented
    /// as no commit rather than failing the run.
    /// [`GitContext::branch`] is informational; the runner does not read it.
    pub git: GitContext,
    /// The local result cache consulted around every fixture.
    ///
    /// Before a fixture runs, the runner looks up the key built from the
    /// fixture, protocol, project and network, and returns a hit instead of
    /// executing the fixture. Results are written back after a successful
    /// execution, subject to the store's [`CachePolicy`](crate::CachePolicy)
    /// (live results are not stored or served by default); execution errors
    /// are never stored.
    pub cache: CacheStore,
    /// Per-run execution knobs.
    ///
    /// The runner reads `max_concurrency` (parallel RPC and Soroban
    /// requests), `rpc_timeout`, `rpc_max_retries`, `rpc_retry_delay_ms` and
    /// `verbose` (per-attempt RPC logging). `quiet` is a reporting setting:
    /// it does not change how fixtures execute.
    pub options: RunOptions,
}
