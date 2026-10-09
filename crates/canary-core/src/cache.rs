//! Local, file-backed result cache.
//!
//! There is deliberately no database here (see the project's database
//! rule): a cache entry is one small JSON file per key under a cache
//! directory, keyed by everything that can invalidate reuse.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::digest::sha256_hex;
use crate::errors::CanaryError;
use crate::model::{CompatibilityResult, ProtocolVersion, ResultSource, Status, Surface};

/// Version of the on-disk entry layout and of the key it was stored under.
///
/// Entries written by an earlier layout (including every entry written
/// before fixture contents were part of the key) are never read: a different
/// file name keeps them from being found and this number must match when an
/// entry is parsed. Bump it whenever what a key means changes.
pub const CACHE_FORMAT: u32 = 3;

/// Everything that must match for a cached result to be safe to reuse.
///
/// No field is an absolute path, so the same project and fixtures produce
/// the same key in any checkout location.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub fixture_id: String,
    /// Digest of the fixture file and any payloads it references, so editing
    /// either one changes the key. See `canary_fixtures::content_digest`.
    pub fixture_digest: String,
    pub protocol: ProtocolVersion,
    pub project_fingerprint: String,
    /// Network name, for example `testnet`.
    pub network: String,
    pub rpc_endpoint: String,
    pub observed_protocol: Option<ProtocolVersion>,
    /// Version of the tool that produced the result: a new release may
    /// change how a fixture is evaluated.
    pub tool_version: String,
}

impl CacheKey {
    /// The full, unambiguous text of this key.
    ///
    /// Strings are written with `{:?}` so separators or newlines inside a
    /// value cannot make two different keys render identically.
    pub fn canonical(&self) -> String {
        format!(
            "canary-cache-key/{CACHE_FORMAT}\nfixture_id={:?}\nfixture_digest={:?}\nprotocol={}\nproject={:?}\nnetwork={:?}\nrpc_endpoint={:?}\nobserved_protocol={}\ntool_version={:?}\n",
            self.fixture_id,
            self.fixture_digest,
            self.protocol.0,
            self.project_fingerprint,
            self.network,
            self.rpc_endpoint,
            self.observed_protocol
                .map(|p| p.0.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            self.tool_version,
        )
    }

    /// A filesystem-safe, fixed-length, deterministic identifier for this key.
    ///
    /// It is a SHA-256 of [`CacheKey::canonical`], so it is stable across
    /// Rust versions and platforms and two different keys cannot share a
    /// name by way of character replacement. The leading `v` and number tie the
    /// name to [`CACHE_FORMAT`].
    pub fn to_file_stem(&self) -> String {
        format!(
            "v{CACHE_FORMAT}-{}",
            sha256_hex(&[self.canonical().as_bytes()])
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    format: u32,
    /// The full canonical key, checked on read so an entry is only ever
    /// returned for the exact key it was written for.
    key: String,
    /// Seconds since the Unix epoch when the entry was written.
    created_at: u64,
    result: CompatibilityResult,
}

/// What the cache may serve and store.
///
/// A cached XDR result is a pure function of the fixture bytes and the
/// pinned `stellar-xdr` version, both of which are in the key, so it can be
/// reused without limit. A cached RPC or Soroban result is a recording of a
/// network's answer at one moment. It says nothing about what the network
/// does now, so it is neither stored nor served unless the caller sets
/// `live_max_age`, and then only while the entry is younger than that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePolicy {
    /// When `false` nothing is read or written (`--no-cache`).
    pub enabled: bool,
    /// Maximum age of a replayed RPC or Soroban result. `None` (the
    /// default) means live results are never replayed.
    pub live_max_age: Option<Duration>,
}

impl Default for CachePolicy {
    fn default() -> Self {
        CachePolicy {
            enabled: true,
            live_max_age: None,
        }
    }
}

impl CachePolicy {
    fn allows(&self, surface: Surface) -> bool {
        self.enabled && (surface == Surface::Xdr || self.live_max_age.is_some())
    }
}

/// A local, file-backed cache of [`CompatibilityResult`]s.
pub struct CacheStore {
    root: PathBuf,
    policy: CachePolicy,
}

impl CacheStore {
    /// A store under `root` with the default [`CachePolicy`]: XDR results
    /// are cached, live results are not.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        CacheStore {
            root: root.into(),
            policy: CachePolicy::default(),
        }
    }

    /// Replaces the policy.
    pub fn with_policy(mut self, policy: CachePolicy) -> Self {
        self.policy = policy;
        self
    }

    fn entry_path(&self, key: &CacheKey) -> PathBuf {
        self.root.join(format!("{}.json", key.to_file_stem()))
    }

    /// Returns a previously cached result for `key`, marked
    /// [`ResultSource::Cache`], if the policy allows it for `surface` and an
    /// acceptable entry exists. Any I/O or parse failure is treated as a
    /// cache miss rather than an error, since a stale/corrupt cache entry
    /// must never turn into a false compatibility failure. An entry whose
    /// layout version or stored key does not match, or that is too old for
    /// a live surface, is also a miss.
    pub fn get(&self, key: &CacheKey, surface: Surface) -> Option<CompatibilityResult> {
        self.get_at(key, surface, SystemTime::now())
    }

    /// [`CacheStore::get`] with the current time supplied by the caller.
    pub fn get_at(
        &self,
        key: &CacheKey,
        surface: Surface,
        now: SystemTime,
    ) -> Option<CompatibilityResult> {
        if !self.policy.allows(surface) {
            return None;
        }
        let bytes = std::fs::read(self.entry_path(key)).ok()?;
        let entry: CacheEntry = serde_json::from_slice(&bytes).ok()?;
        if entry.format != CACHE_FORMAT || entry.key != key.canonical() {
            return None;
        }
        if surface != Surface::Xdr {
            let max_age = self.policy.live_max_age?;
            let created = UNIX_EPOCH + Duration::from_secs(entry.created_at);
            // An entry stamped in the future (a clock that moved back, or a
            // hand-edited file) has no trustworthy age, so it is not used.
            let age = now.duration_since(created).ok()?;
            if age > max_age {
                return None;
            }
        }
        let mut result = entry.result;
        result.source = ResultSource::Cache;
        Some(result)
    }

    /// Stores `result` under `key` if the policy allows it for `surface`,
    /// unless it is an execution error: temporary execution failures (e.g.
    /// an RPC timeout) must not be cached as if they were a stable outcome.
    pub fn put(
        &self,
        key: &CacheKey,
        surface: Surface,
        result: &CompatibilityResult,
    ) -> Result<(), CanaryError> {
        if result.status == Status::Error || !self.policy.allows(surface) {
            return Ok(());
        }
        std::fs::create_dir_all(&self.root)
            .map_err(|e| CanaryError::Cache(format!("failed to create cache directory: {e}")))?;
        let entry = CacheEntry {
            format: CACHE_FORMAT,
            key: key.canonical(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            result: CompatibilityResult {
                // What is stored is a recording of a live execution.
                source: ResultSource::Live,
                ..result.clone()
            },
        };
        let bytes = serde_json::to_vec_pretty(&entry)
            .map_err(|e| CanaryError::Cache(format!("failed to serialize cache entry: {e}")))?;
        // Write to a temporary name and rename, so a reader (or a second
        // `check` in the same directory) never sees a half-written entry.
        let target = self.entry_path(key);
        let temp = target.with_extension(format!("tmp-{}", std::process::id()));
        std::fs::write(&temp, bytes)
            .and_then(|()| std::fs::rename(&temp, &target))
            .map_err(|e| {
                let _ = std::fs::remove_file(&temp);
                CanaryError::Cache(format!("failed to write cache entry: {e}"))
            })?;
        Ok(())
    }

    /// Removes every cached entry.
    pub fn clear(&self) -> Result<(), CanaryError> {
        if !self.root.exists() {
            return Ok(());
        }
        std::fs::remove_dir_all(&self.root)
            .map_err(|e| CanaryError::Cache(format!("failed to clear cache directory: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Surface;

    fn sample_key() -> CacheKey {
        CacheKey {
            fixture_id: "p28-xdr-cap83-001".into(),
            fixture_digest: "digest-a".into(),
            protocol: ProtocolVersion(28),
            project_fingerprint: "abc123".into(),
            network: "testnet".into(),
            rpc_endpoint: "https://soroban-testnet.stellar.org".into(),
            observed_protocol: Some(ProtocolVersion(28)),
            tool_version: "0.1.1".into(),
        }
    }

    fn sample_result(status: Status) -> CompatibilityResult {
        CompatibilityResult {
            test_id: "p28-xdr-cap83-001".into(),
            protocol: ProtocolVersion(28),
            surface: Surface::Xdr,
            status,
            summary: "ok".into(),
            details: None,
            duration_ms: 5,
            fixture_id: Some("p28-xdr-cap83-001".into()),
            source: crate::model::ResultSource::Live,
        }
    }

    #[test]
    fn round_trips_a_stored_result() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        assert!(store.get(&key, Surface::Xdr).is_none());

        store
            .put(&key, Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();
        let fetched = store.get(&key, Surface::Xdr).expect("cached result");
        assert_eq!(fetched.status, Status::Pass);
    }

    #[test]
    fn does_not_cache_execution_errors() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();

        store
            .put(&key, Surface::Xdr, &sample_result(Status::Error))
            .unwrap();
        assert!(store.get(&key, Surface::Xdr).is_none());
    }

    #[test]
    fn clear_removes_all_entries() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        store
            .put(&key, Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();
        assert!(store.get(&key, Surface::Xdr).is_some());

        store.clear().unwrap();
        assert!(store.get(&key, Surface::Xdr).is_none());
    }

    #[test]
    fn every_key_field_changes_the_file_stem() {
        let base = sample_key().to_file_stem();
        let variants: Vec<(&str, CacheKey)> = vec![
            (
                "fixture_id",
                CacheKey {
                    fixture_id: "other".into(),
                    ..sample_key()
                },
            ),
            (
                "fixture_digest",
                CacheKey {
                    fixture_digest: "digest-b".into(),
                    ..sample_key()
                },
            ),
            (
                "protocol",
                CacheKey {
                    protocol: ProtocolVersion(29),
                    ..sample_key()
                },
            ),
            (
                "project",
                CacheKey {
                    project_fingerprint: "other".into(),
                    ..sample_key()
                },
            ),
            (
                "network",
                CacheKey {
                    network: "mainnet".into(),
                    ..sample_key()
                },
            ),
            (
                "rpc_endpoint",
                CacheKey {
                    rpc_endpoint: "https://b.example".into(),
                    ..sample_key()
                },
            ),
            (
                "observed",
                CacheKey {
                    observed_protocol: Some(ProtocolVersion(29)),
                    ..sample_key()
                },
            ),
            (
                "observed none",
                CacheKey {
                    observed_protocol: None,
                    ..sample_key()
                },
            ),
            (
                "tool_version",
                CacheKey {
                    tool_version: "0.2.0".into(),
                    ..sample_key()
                },
            ),
        ];
        for (name, key) in variants {
            assert_ne!(key.to_file_stem(), base, "{name} must be part of the key");
        }
    }

    #[test]
    fn file_stem_is_fixed_length_and_filesystem_safe() {
        let key = CacheKey {
            fixture_id: "a/b\\c:d e\n..".into(),
            ..sample_key()
        };
        let stem = key.to_file_stem();
        assert_eq!(stem.len(), "v3-".len() + 64);
        assert!(stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn ids_that_only_differ_in_punctuation_do_not_collide() {
        // The previous scheme replaced every non-alphanumeric character
        // with '_', so these two fixtures shared one cache file.
        let a = CacheKey {
            fixture_id: "a.b".into(),
            ..sample_key()
        };
        let b = CacheKey {
            fixture_id: "a_b".into(),
            ..sample_key()
        };
        assert_ne!(a.to_file_stem(), b.to_file_stem());
    }

    #[test]
    fn a_changed_fixture_digest_misses_the_previous_entry() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let before = sample_key();
        store
            .put(&before, Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();

        let after = CacheKey {
            fixture_digest: "digest-b".into(),
            ..before.clone()
        };
        assert!(store.get(&after, Surface::Xdr).is_none());
        assert!(
            store.get(&before, Surface::Xdr).is_some(),
            "the original key still hits"
        );
    }

    #[test]
    fn an_entry_in_the_old_layout_is_never_read() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        // Shape written before fixture contents were part of the key: no
        // `format`, no `key`, only the result, under the new file name.
        let legacy = serde_json::json!({ "result": sample_result(Status::Pass) });
        std::fs::write(
            dir.path().join(format!("{}.json", key.to_file_stem())),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        assert!(store.get(&key, Surface::Xdr).is_none());
    }

    #[test]
    fn an_entry_with_another_format_number_is_not_read() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        store
            .put(&key, Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();
        let path = dir.path().join(format!("{}.json", key.to_file_stem()));
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["format"] = serde_json::json!(CACHE_FORMAT + 1);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(store.get(&key, Surface::Xdr).is_none());
    }

    #[test]
    fn an_entry_copied_under_another_keys_name_is_not_returned() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        let other = CacheKey {
            fixture_digest: "digest-b".into(),
            ..sample_key()
        };
        store
            .put(&key, Surface::Xdr, &sample_result(Status::Fail))
            .unwrap();
        std::fs::copy(
            dir.path().join(format!("{}.json", key.to_file_stem())),
            dir.path().join(format!("{}.json", other.to_file_stem())),
        )
        .unwrap();
        assert!(store.get(&other, Surface::Xdr).is_none());
    }

    #[test]
    fn a_corrupted_entry_is_a_miss_not_an_error() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        std::fs::write(
            dir.path().join(format!("{}.json", key.to_file_stem())),
            b"{ not json",
        )
        .unwrap();
        assert!(store.get(&key, Surface::Xdr).is_none());
        // And it can be replaced by a good entry afterwards.
        store
            .put(&key, Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();
        assert!(store.get(&key, Surface::Xdr).is_some());
    }

    #[test]
    fn no_temporary_file_is_left_behind_after_a_write() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        store
            .put(&sample_key(), Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].ends_with(".json"));
    }

    fn live_store(dir: &TempDir, max_age_secs: u64) -> CacheStore {
        CacheStore::new(dir.path()).with_policy(CachePolicy {
            enabled: true,
            live_max_age: Some(Duration::from_secs(max_age_secs)),
        })
    }

    #[test]
    fn a_replayed_result_is_marked_as_coming_from_the_cache() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        store
            .put(&key, Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();

        assert_eq!(
            store.get(&key, Surface::Xdr).unwrap().source,
            ResultSource::Cache
        );
    }

    #[test]
    fn a_stored_result_is_recorded_as_a_live_execution() {
        // Whatever marker the caller's value carried, the file records the
        // execution it came from; only reading it back says `cache`.
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        let replayed = CompatibilityResult {
            source: ResultSource::Cache,
            ..sample_result(Status::Pass)
        };
        store.put(&key, Surface::Xdr, &replayed).unwrap();
        let raw = std::fs::read_to_string(dir.path().join(format!("{}.json", key.to_file_stem())))
            .unwrap();
        assert!(raw.contains("\"source\": \"live\""), "{raw}");
    }

    #[test]
    fn live_results_are_neither_stored_nor_served_by_default() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        for surface in [Surface::Rpc, Surface::Soroban] {
            store
                .put(&key, surface, &sample_result(Status::Pass))
                .unwrap();
            assert!(store.get(&key, surface).is_none(), "{surface}");
        }
        assert!(
            std::fs::read_dir(dir.path())
                .map(|mut d| d.next().is_none())
                .unwrap_or(true),
            "nothing was written for live surfaces"
        );
    }

    #[test]
    fn a_live_entry_is_served_only_while_it_is_fresh() {
        let dir = tempdir();
        let store = live_store(&dir, 60);
        let key = sample_key();
        store
            .put(&key, Surface::Rpc, &sample_result(Status::Pass))
            .unwrap();
        let written = SystemTime::now();

        let at = |secs: u64| store.get_at(&key, Surface::Rpc, written + Duration::from_secs(secs));
        assert!(at(0).is_some(), "just written");
        assert!(at(59).is_some(), "inside the window");
        assert!(at(120).is_none(), "outside the window");
        assert_eq!(at(1).unwrap().source, ResultSource::Cache);
    }

    #[test]
    fn a_live_entry_dated_in_the_future_is_not_trusted() {
        let dir = tempdir();
        let store = live_store(&dir, 3600);
        let key = sample_key();
        store
            .put(&key, Surface::Soroban, &sample_result(Status::Pass))
            .unwrap();

        let long_ago = SystemTime::now() - Duration::from_secs(7200);
        assert!(store.get_at(&key, Surface::Soroban, long_ago).is_none());
    }

    #[test]
    fn xdr_results_do_not_expire() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        store
            .put(&key, Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();

        let far_future = SystemTime::now() + Duration::from_secs(365 * 24 * 3600);
        assert!(store.get_at(&key, Surface::Xdr, far_future).is_some());
    }

    #[test]
    fn a_disabled_cache_reads_and_writes_nothing() {
        let dir = tempdir();
        let enabled = CacheStore::new(dir.path());
        let key = sample_key();
        enabled
            .put(&key, Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();

        let disabled = CacheStore::new(dir.path()).with_policy(CachePolicy {
            enabled: false,
            live_max_age: Some(Duration::from_secs(60)),
        });
        assert!(
            disabled.get(&key, Surface::Xdr).is_none(),
            "an existing entry is not read"
        );
        let other = CacheKey {
            fixture_id: "other".into(),
            ..key
        };
        disabled
            .put(&other, Surface::Xdr, &sample_result(Status::Pass))
            .unwrap();
        assert!(
            enabled.get(&other, Surface::Xdr).is_none(),
            "nothing was written"
        );
    }

    #[test]
    fn an_entry_without_a_creation_time_is_never_read() {
        // Layout 2 (before replay provenance and expiry) had no timestamp.
        let dir = tempdir();
        let store = live_store(&dir, 3600);
        let key = sample_key();
        let layout2 = serde_json::json!({
            "format": 2,
            "key": key.canonical(),
            "result": sample_result(Status::Pass),
        });
        std::fs::write(
            dir.path().join(format!("{}.json", key.to_file_stem())),
            serde_json::to_vec(&layout2).unwrap(),
        )
        .unwrap();
        assert!(store.get(&key, Surface::Xdr).is_none());
        assert!(store.get(&key, Surface::Rpc).is_none());
    }

    /// Minimal temp-dir helper so this crate does not need a `tempfile`
    /// dev-dependency for a handful of cache tests.
    ///
    /// Combines a nanosecond timestamp with a per-process atomic counter:
    /// the timestamp alone is not a real uniqueness guarantee across
    /// threads run in parallel by the test harness, since clock
    /// resolution on some (especially virtualized) hosts can be coarser
    /// than the interval between two threads' reads — a collision here
    /// previously let one test's `clear()` (`remove_dir_all`) delete
    /// another concurrently-running test's cache directory mid-test.
    fn tempdir() -> TempDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let mut path = std::env::temp_dir();
        let unique = format!(
            "canary-core-cache-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        path.push(unique);
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn path(&self) -> &PathBuf {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
