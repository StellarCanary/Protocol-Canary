//! Local, file-backed result cache.
//!
//! There is deliberately no database here (see the project's database
//! rule): a cache entry is one small JSON file per key under a cache
//! directory, keyed by everything that can invalidate reuse.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::errors::CanaryError;
use crate::model::{CompatibilityResult, ProtocolVersion, Status};

/// Everything that must match for a cached result to be safe to reuse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub fixture_id: String,
    pub protocol: ProtocolVersion,
    pub project_fingerprint: String,
    pub rpc_endpoint: String,
    pub observed_protocol: Option<ProtocolVersion>,
}

impl CacheKey {
    /// A filesystem-safe, deterministic identifier for this key.
    ///
    /// The RPC endpoint may contain characters that are not safe in a file
    /// name, so it is hashed rather than embedded verbatim; every other
    /// field is embedded directly so the file name stays inspectable.
    pub fn to_file_stem(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.rpc_endpoint.hash(&mut hasher);
        let endpoint_hash = hasher.finish();
        format!(
            "{}__p{}__{}__{:016x}__{}",
            sanitize(&self.fixture_id),
            self.protocol.0,
            sanitize(&self.project_fingerprint),
            endpoint_hash,
            self.observed_protocol
                .map(|p| p.0.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        )
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    result: CompatibilityResult,
}

/// A local, file-backed cache of [`CompatibilityResult`]s.
pub struct CacheStore {
    root: PathBuf,
}

impl CacheStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        CacheStore { root: root.into() }
    }

    fn entry_path(&self, key: &CacheKey) -> PathBuf {
        self.root.join(format!("{}.json", key.to_file_stem()))
    }

    /// Returns a previously cached result for `key`, if one exists and is
    /// readable. Any I/O or parse failure is treated as a cache miss rather
    /// than an error, since a stale/corrupt cache entry must never turn
    /// into a false compatibility failure.
    pub fn get(&self, key: &CacheKey) -> Option<CompatibilityResult> {
        let bytes = std::fs::read(self.entry_path(key)).ok()?;
        let entry: CacheEntry = serde_json::from_slice(&bytes).ok()?;
        Some(entry.result)
    }

    /// Stores `result` under `key`, unless it is an execution error:
    /// temporary execution failures (e.g. an RPC timeout) must not be
    /// cached as if they were a stable outcome.
    pub fn put(&self, key: &CacheKey, result: &CompatibilityResult) -> Result<(), CanaryError> {
        if result.status == Status::Error {
            return Ok(());
        }
        std::fs::create_dir_all(&self.root)
            .map_err(|e| CanaryError::Cache(format!("failed to create cache directory: {e}")))?;
        let entry = CacheEntry {
            result: result.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&entry)
            .map_err(|e| CanaryError::Cache(format!("failed to serialize cache entry: {e}")))?;
        std::fs::write(self.entry_path(key), bytes)
            .map_err(|e| CanaryError::Cache(format!("failed to write cache entry: {e}")))?;
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
            protocol: ProtocolVersion(28),
            project_fingerprint: "abc123".into(),
            rpc_endpoint: "https://soroban-testnet.stellar.org".into(),
            observed_protocol: Some(ProtocolVersion(28)),
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
        }
    }

    #[test]
    fn round_trips_a_stored_result() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        assert!(store.get(&key).is_none());

        store.put(&key, &sample_result(Status::Pass)).unwrap();
        let fetched = store.get(&key).expect("cached result");
        assert_eq!(fetched.status, Status::Pass);
    }

    #[test]
    fn does_not_cache_execution_errors() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();

        store.put(&key, &sample_result(Status::Error)).unwrap();
        assert!(store.get(&key).is_none());
    }

    #[test]
    fn clear_removes_all_entries() {
        let dir = tempdir();
        let store = CacheStore::new(dir.path());
        let key = sample_key();
        store.put(&key, &sample_result(Status::Pass)).unwrap();
        assert!(store.get(&key).is_some());

        store.clear().unwrap();
        assert!(store.get(&key).is_none());
    }

    #[test]
    fn different_rpc_endpoints_produce_different_cache_entries() {
        let mut key_a = sample_key();
        let mut key_b = sample_key();
        key_a.rpc_endpoint = "https://a.example".into();
        key_b.rpc_endpoint = "https://b.example".into();
        assert_ne!(key_a.to_file_stem(), key_b.to_file_stem());
    }

    /// Minimal temp-dir helper so this crate does not need a `tempfile`
    /// dev-dependency for a handful of cache tests.
    fn tempdir() -> TempDir {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "canary-core-cache-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
