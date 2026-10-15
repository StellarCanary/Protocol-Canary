//! Configuration loading and validation for Stellar Protocol Canary.

pub mod loader;
pub mod schema;

pub use loader::{
    load, load_from_root, load_lock, parse_lock, serialize_lock, ConfigError, LockError,
    CONFIG_FILE_NAME, LOCK_FILE_NAME,
};
pub use schema::{
    ConfigFile, LockFile, LockFixtures, LockSource, LockTool, PolicySection, ProjectSection,
    ProjectTypeSetting, TestsSection, DEFAULT_PROTOCOL, MAX_LOCK_FILE_BYTES,
    SUPPORTED_CONFIG_VERSION, SUPPORTED_LOCK_VERSION, SUPPORTED_REGISTRY_VERSION,
};
