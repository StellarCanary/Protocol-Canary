//! Loading and validating `.stellar-canary.toml`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use canary_core::CanaryError;

use crate::schema::{
    ConfigFile, LockFile, MAX_LOCK_FILE_BYTES, SUPPORTED_CONFIG_VERSION, SUPPORTED_LOCK_VERSION,
};

/// The default configuration file name looked for in a project root.
pub const CONFIG_FILE_NAME: &str = ".stellar-canary.toml";

/// The lockfile name, next to the configuration file in a project root
/// (CF-03 section 2).
pub const LOCK_FILE_NAME: &str = ".stellar-canary.lock";

/// The temporary file a lock writer creates next to the lockfile before
/// renaming it into place (CF-03 section 6).
///
/// The name is fixed rather than unique: creating it exclusively is what
/// gives one process at a time the right to write the lock, and a second
/// concurrent writer fails instead of interleaving with the first.
pub const LOCK_TEMP_FILE_NAME: &str = ".stellar-canary.lock.tmp";

/// The network used when neither the CLI nor `.stellar-canary.toml` names
/// one (see [`resolve_settings`]).
pub const DEFAULT_NETWORK: &str = "testnet";

/// The fixture directory used when neither the CLI nor
/// `.stellar-canary.toml` names one (see [`resolve_settings`]).
pub const DEFAULT_FIXTURES_DIR: &str = "fixtures";

#[derive(Debug, thiserror::Error)]
/// Errors returned while reading, parsing, or validating a Canary configuration.
///
/// Each variant retains the configuration path involved in the failure where
/// applicable, so callers can report actionable diagnostics to the contributor
/// who owns that project configuration.
pub enum ConfigError {
    #[error("failed to read configuration file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse configuration file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("unsupported configuration version {found} in {path}: this build supports version {SUPPORTED_CONFIG_VERSION}")]
    UnsupportedVersion { path: PathBuf, found: u32 },

    #[error("invalid configuration in {path}: {reason}")]
    Invalid { path: PathBuf, reason: String },
}

impl From<ConfigError> for CanaryError {
    fn from(error: ConfigError) -> Self {
        CanaryError::Configuration(error.to_string())
    }
}

/// Loads and validates a configuration file at an explicit path.
pub fn load(path: &Path) -> Result<ConfigFile, ConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&raw, path)
}

/// Looks for `.stellar-canary.toml` in `root` and loads it if present.
///
/// Returns `Ok(None)` (not an error) when the file does not exist: a
/// project with no configuration file uses the built-in defaults rather
/// than failing.
pub fn load_from_root(root: &Path) -> Result<Option<ConfigFile>, ConfigError> {
    let path = root.join(CONFIG_FILE_NAME);
    if !path.exists() {
        return Ok(None);
    }
    load(&path).map(Some)
}

fn parse(raw: &str, path: &Path) -> Result<ConfigFile, ConfigError> {
    let config: ConfigFile = toml::from_str(raw).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    validate(&config, path)?;
    Ok(config)
}

fn validate(config: &ConfigFile, path: &Path) -> Result<(), ConfigError> {
    if config.version != SUPPORTED_CONFIG_VERSION {
        return Err(ConfigError::UnsupportedVersion {
            path: path.to_path_buf(),
            found: config.version,
        });
    }
    if config.protocol == 0 {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: "protocol must be a positive protocol version number".to_string(),
        });
    }
    if !(config.tests.xdr || config.tests.rpc || config.tests.soroban) {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: "at least one of [tests].xdr, [tests].rpc, [tests].soroban must be enabled"
                .to_string(),
        });
    }
    Ok(())
}

/// The CLI values a user explicitly passed, for cascading over
/// `.stellar-canary.toml` (issue #474).
///
/// `None` means "not passed": the configuration file, then the built-in
/// default, supplies the value. Commands keep `--network` and
/// `--fixtures-dir` as `Option`s on the argument parser precisely so an
/// explicit override stays distinguishable from a clap default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CliOverrides {
    pub network: Option<String>,
    pub rpc_url: Option<String>,
    pub fixtures_dir: Option<PathBuf>,
}

/// A run's settings after cascading CLI > `.stellar-canary.toml` >
/// built-in defaults, resolved per key (issue #474).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSettings {
    pub network: String,
    /// `None` when neither source names an endpoint. Whether the resolved
    /// network then has a built-in default RPC URL is decided by the
    /// caller — in `canary-cli`, only `testnet` does.
    pub rpc_url: Option<String>,
    pub fixtures_dir: PathBuf,
}

/// Resolves `network`, `rpc_url` and `fixtures_dir` by cascading the
/// explicitly passed CLI values over the configuration file's settings,
/// and those over the built-in defaults (`"testnet"`, none, `"fixtures"`).
///
/// Precedence is applied per key, so passing only `--network` still lets
/// `rpc_url` and `fixtures_dir` come from the configuration file. Values
/// are taken verbatim: an unusable `rpc_url` recorded in the
/// configuration file does not stop a valid `--rpc-url` from winning, and
/// is never validated here.
pub fn resolve_settings(config: &ConfigFile, cli: CliOverrides) -> ResolvedSettings {
    ResolvedSettings {
        network: cli
            .network
            .or_else(|| config.network.clone())
            .unwrap_or_else(|| DEFAULT_NETWORK.to_string()),
        rpc_url: cli.rpc_url.or_else(|| config.rpc_url.clone()),
        fixtures_dir: cli
            .fixtures_dir
            .or_else(|| config.fixtures_dir.clone())
            .unwrap_or_else(|| PathBuf::from(DEFAULT_FIXTURES_DIR)),
    }
}

/// Why a lockfile could not be read, parsed, validated or written.
///
/// Every variant is a configuration problem: a lock is a pin supplied by the
/// project, so a bad one stops the run with exit code 2 before anything
/// executes, and is never repaired or ignored.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("failed to read lockfile {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("lockfile {path} is a symbolic link; a lockfile must be a regular file")]
    Symlink { path: PathBuf },

    #[error("lockfile {path} is not a regular file")]
    NotRegularFile { path: PathBuf },

    #[error("lockfile {path} is {size} bytes, which exceeds the {limit} byte limit")]
    TooLarge {
        path: PathBuf,
        size: u64,
        limit: u64,
    },

    #[error("lockfile {path} is not valid UTF-8")]
    NotUtf8 { path: PathBuf },

    #[error("failed to parse lockfile {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("unsupported lockVersion {found} in {path}: this build supports version {supported}")]
    UnsupportedVersion {
        path: PathBuf,
        found: i64,
        supported: u32,
    },

    #[error("invalid lockfile {path}: {reason}")]
    Invalid { path: PathBuf, reason: String },

    #[error("failed to write lockfile {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "lockfile {path} is being written by another process; retry once {temp} no longer exists"
    )]
    Busy { path: PathBuf, temp: PathBuf },
}

impl From<LockError> for CanaryError {
    fn from(error: LockError) -> Self {
        CanaryError::Configuration(error.to_string())
    }
}

/// Reads and validates the lockfile at `path` (CF-03 sections 2 and 3).
///
/// Rejected before any parsing: a symbolic link, anything that is not a
/// regular file, a file over 64 KiB, and non-UTF-8 content. The size limit is
/// enforced on the bytes actually read as well as on the reported length, so
/// a file that grows while it is being read cannot exceed it.
pub fn load_lock(path: &Path) -> Result<LockFile, LockError> {
    let read_error = |source| LockError::Read {
        path: path.to_path_buf(),
        source,
    };

    let metadata = std::fs::symlink_metadata(path).map_err(read_error)?;
    if metadata.file_type().is_symlink() {
        return Err(LockError::Symlink {
            path: path.to_path_buf(),
        });
    }
    if !metadata.is_file() {
        return Err(LockError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }
    if metadata.len() > MAX_LOCK_FILE_BYTES {
        return Err(LockError::TooLarge {
            path: path.to_path_buf(),
            size: metadata.len(),
            limit: MAX_LOCK_FILE_BYTES,
        });
    }

    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(read_error)?
        .take(MAX_LOCK_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| LockError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() as u64 > MAX_LOCK_FILE_BYTES {
        return Err(LockError::TooLarge {
            path: path.to_path_buf(),
            size: bytes.len() as u64,
            limit: MAX_LOCK_FILE_BYTES,
        });
    }

    let text = String::from_utf8(bytes).map_err(|_| LockError::NotUtf8 {
        path: path.to_path_buf(),
    })?;
    parse_lock(&text, path)
}

/// Parses and validates lockfile text. `path` is only used in diagnostics.
///
/// `lockVersion` is checked first, on its own, so a file written by a newer
/// version is reported as an unsupported version rather than as whichever of
/// its other keys this build happens not to know.
pub fn parse_lock(raw: &str, path: &Path) -> Result<LockFile, LockError> {
    if raw.len() as u64 > MAX_LOCK_FILE_BYTES {
        return Err(LockError::TooLarge {
            path: path.to_path_buf(),
            size: raw.len() as u64,
            limit: MAX_LOCK_FILE_BYTES,
        });
    }

    let table: toml::Table = toml::from_str(raw).map_err(|source| LockError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;

    let invalid = |reason: String| LockError::Invalid {
        path: path.to_path_buf(),
        reason,
    };
    match table.get("lockVersion") {
        Some(toml::Value::Integer(found)) if *found == i64::from(SUPPORTED_LOCK_VERSION) => {}
        Some(toml::Value::Integer(found)) => {
            return Err(LockError::UnsupportedVersion {
                path: path.to_path_buf(),
                found: *found,
                supported: SUPPORTED_LOCK_VERSION,
            });
        }
        Some(other) => {
            return Err(invalid(format!(
                "lockVersion must be the integer {SUPPORTED_LOCK_VERSION}, found {}",
                toml_kind(other)
            )));
        }
        None => return Err(invalid("missing required key \"lockVersion\"".to_string())),
    }

    let lock: LockFile =
        toml::Value::Table(table)
            .try_into()
            .map_err(|source| LockError::Parse {
                path: path.to_path_buf(),
                source: Box::new(source),
            })?;
    lock.validate().map_err(invalid)?;
    Ok(lock)
}

/// Renders a lock in its one canonical form (CF-03 section 3).
///
/// Keys are written in the order of the contract, strings are always
/// double-quoted, lines end in `\n` with one trailing newline, and nothing
/// else is emitted: no comments, no header, no trailing whitespace. Identical
/// inputs give byte-identical output, and `parse_lock` of the output
/// reproduces the input.
///
/// The lock is validated first, so a value that would be rejected on read is
/// an error here instead of a file nobody can load.
pub fn serialize_lock(lock: &LockFile) -> Result<String, LockError> {
    lock.validate().map_err(|reason| LockError::Invalid {
        path: PathBuf::from(LOCK_FILE_NAME),
        reason,
    })?;

    let mut lines = vec![
        format!("lockVersion = {}", lock.lock_version),
        String::new(),
    ];
    if let Some(tool) = &lock.tool {
        lines.push("[tool]".to_string());
        lines.push(format!("version = {}", toml_string(&tool.version)));
        lines.push(String::new());
    }
    let fixtures = &lock.fixtures;
    lines.push("[fixtures]".to_string());
    lines.push(format!("protocol = {}", fixtures.protocol));
    lines.push(format!(
        "packDigest = {}",
        toml_string(&fixtures.pack_digest)
    ));
    lines.push(format!("registryVersion = {}", fixtures.registry_version));
    if let Some(source) = fixtures.source {
        lines.push(format!("source = {}", toml_string(source.as_str())));
    }
    if let Some(revision) = &fixtures.revision {
        lines.push(format!("revision = {}", toml_string(revision)));
    }

    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

/// Serializes `lock` and writes it to `path` atomically (CF-03 section 6).
///
/// The lock is validated and rendered first, so a lock that could not be
/// read back never touches the filesystem. The canonical bytes are then
/// written to a temporary file in the same directory as `path`, flushed and
/// synced, given mode `0644`, and renamed over the target. The rename is
/// atomic on a POSIX filesystem, so a crash leaves either the old lock or
/// the complete new one, never a zero-byte or partially written file.
///
/// Mutual exclusion comes from creating the fixed-name temporary file at
/// [`LOCK_TEMP_FILE_NAME`] with `create_new`: if another writer holds it,
/// this call fails with [`LockError::Busy`] rather than waiting or
/// overwriting it. A symbolic link at either the target or the temporary
/// path is refused, and the temporary file is removed if writing or
/// renaming fails.
///
/// # Errors
///
/// Returns [`LockError::Invalid`] if `lock` fails validation, before any
/// file is created. Returns [`LockError::Symlink`] for a symbolic link,
/// [`LockError::Busy`] when another writer holds the temporary file, and
/// [`LockError::Write`] for any other filesystem failure.
pub fn write_lock(path: &Path, lock: &LockFile) -> Result<(), LockError> {
    // Serialize first: a rejected lock must not replace a good file.
    let text = serialize_lock(lock)?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| LockError::Invalid {
            path: path.to_path_buf(),
            reason: "lockfile path must name a UTF-8 file".to_string(),
        })?;
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let temp = parent.join(format!("{file_name}.tmp"));

    // The target may be an existing lock, but never a link or a directory.
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(LockError::Symlink {
                path: path.to_path_buf(),
            });
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(LockError::NotRegularFile {
                path: path.to_path_buf(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(LockError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    }

    // Refuse to follow a pre-existing link at the temporary path, then claim
    // it exclusively. `create_new` is the lock: only one writer can hold it.
    if std::fs::symlink_metadata(&temp).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(LockError::Symlink { path: temp });
    }
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
    {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(LockError::Busy {
                path: path.to_path_buf(),
                temp,
            });
        }
        Err(source) => {
            return Err(LockError::Write { path: temp, source });
        }
    };

    // Remove the temporary file on every failure path. `file_name` is kept
    // separate from `temp` only to make this keyword-free guard readable.
    let result = (|| {
        file.write_all(text.as_bytes())
            .map_err(|source| LockError::Write {
                path: temp.clone(),
                source,
            })?;
        file.flush().map_err(|source| LockError::Write {
            path: temp.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| LockError::Write {
            path: temp.clone(),
            source,
        })?;
        set_lock_permissions(&file, &temp)?;
        drop(file);
        std::fs::rename(&temp, path).map_err(|source| LockError::Write {
            path: path.to_path_buf(),
            source,
        })
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Gives a freshly created lockfile mode `0644` on Unix. Other platforms
/// inherit the permissions a newly created file normally receives.
fn set_lock_permissions(file: &std::fs::File, path: &Path) -> Result<(), LockError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o644))
            .map_err(|source| LockError::Write {
                path: path.to_path_buf(),
                source,
            })?;
    }
    #[cfg(not(unix))]
    {
        let _ = (file, path);
    }
    Ok(())
}

/// A TOML basic string. Always double-quoted, so the writer never switches
/// quoting style depending on the content.
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn toml_kind(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "a string",
        toml::Value::Integer(_) => "an integer",
        toml::Value::Float(_) => "a float",
        toml::Value::Boolean(_) => "a boolean",
        toml::Value::Datetime(_) => "a datetime",
        toml::Value::Array(_) => "an array",
        toml::Value::Table(_) => "a table",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ProjectTypeSetting;

    fn write_temp_config(contents: &str) -> (tempdir::TempDir, PathBuf) {
        let dir = tempdir::TempDir::new("canary-config-test");
        let path = dir.path.join(CONFIG_FILE_NAME);
        std::fs::write(&path, contents).unwrap();
        (dir, path)
    }

    #[test]
    fn loads_the_documented_mvp_example() {
        let (_dir, path) = write_temp_config(
            r#"
            version = 1
            protocol = 28

            [project]
            type = "auto"

            [tests]
            xdr = true
            rpc = true
            soroban = true

            [policy]
            warnings_are_failures = false
            "#,
        );

        let config = load(&path).expect("valid config");
        assert_eq!(config.version, 1);
        assert_eq!(config.protocol, 28);
        assert_eq!(config.project.project_type, ProjectTypeSetting::Auto);
    }

    #[test]
    fn missing_file_returns_none_rather_than_an_error() {
        let dir = tempdir::TempDir::new("canary-config-missing");
        let result = load_from_root(&dir.path).expect("no io error");
        assert!(result.is_none());
    }

    #[test]
    fn load_from_root_discovers_and_parses_an_existing_config_file() {
        use canary_core::ProjectType;

        let dir = tempdir::TempDir::new("canary-config-found");
        // The literal on-disk name rather than `CONFIG_FILE_NAME`, so this
        // pins the discovery contract instead of restating it.
        std::fs::write(
            dir.path.join(".stellar-canary.toml"),
            r#"
            version = 1
            protocol = 22

            [project]
            type = "soroban"

            [tests]
            xdr = true
            rpc = false
            soroban = true

            [policy]
            warnings_are_failures = true
            "#,
        )
        .unwrap();

        let config = load_from_root(&dir.path)
            .expect("an existing config file must load, not error")
            .expect("a config file at the conventional name must be discovered");

        // Values that differ from the schema defaults, so this fails if the
        // parsed file were silently replaced by `ConfigFile::default()`.
        assert_eq!(config.version, 1);
        assert_eq!(config.protocol, 22);
        assert_eq!(
            config.project.project_type,
            ProjectTypeSetting::Explicit(ProjectType::Soroban)
        );
        assert!(config.tests.xdr);
        assert!(!config.tests.rpc);
        assert!(config.tests.soroban);
        assert!(config.policy.warnings_are_failures);
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let (_dir, path) = write_temp_config("version = 2\nprotocol = 28\n");
        let err = load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::UnsupportedVersion { .. }));
    }

    #[test]
    fn rejects_zero_protocol() {
        let (_dir, path) = write_temp_config("version = 1\nprotocol = 0\n");
        let err = load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
    }

    #[test]
    fn rejects_all_surfaces_disabled() {
        let (_dir, path) = write_temp_config(
            r#"
            version = 1
            protocol = 28

            [tests]
            xdr = false
            rpc = false
            soroban = false
            "#,
        );
        let err = load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
    }

    #[test]
    fn rejects_a_misspelled_section_instead_of_silently_ignoring_it() {
        // `[test]` for `[tests]` used to leave every surface enabled and say
        // nothing, so a project meaning to run offline made network calls.
        let (_dir, path) = write_temp_config("version = 1\nprotocol = 28\n\n[test]\nrpc = false\n");
        let err = load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
        assert!(err.to_string().contains("unknown field `test`"), "{err}");
    }

    #[test]
    fn rejects_a_misspelled_key_inside_a_section() {
        let (_dir, path) = write_temp_config(
            "version = 1\nprotocol = 28\n\n[policy]\nwarnings_are_failure = true\n",
        );
        let err = load(&path).unwrap_err();
        assert!(
            err.to_string()
                .contains("unknown field `warnings_are_failure`"),
            "{err}"
        );
    }

    #[test]
    fn rejects_an_unknown_top_level_key() {
        // `networks` is a misspelling of the now-supported `network` key:
        // deny_unknown_fields must still reject it outright.
        let (_dir, path) =
            write_temp_config("version = 1\nprotocol = 28\nnetworks = \"mainnet\"\n");
        let err = load(&path).unwrap_err();
        assert!(
            err.to_string().contains("unknown field `networks`"),
            "{err}"
        );
    }

    #[test]
    fn rejects_malformed_toml() {
        let (_dir, path) = write_temp_config("this is not valid toml [[[");
        let err = load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn config_errors_map_to_the_configuration_canary_error_variant() {
        let (_dir, path) = write_temp_config("version = 2\nprotocol = 28\n");
        let err = load(&path).unwrap_err();
        let canary_err: CanaryError = err.into();
        assert!(matches!(canary_err, CanaryError::Configuration(_)));
    }

    /// Minimal temp-dir helper, avoiding a `tempfile` dev-dependency for a
    /// handful of config-loading tests.
    ///
    /// The name combines the process id, a nanosecond timestamp and a
    /// per-process atomic counter. The timestamp alone is not a uniqueness
    /// guarantee across the threads the test harness runs in parallel, since
    /// clock resolution on some hosts is coarser than the interval between two
    /// threads' reads — a collision made two tests share one directory, and
    /// one test's `Drop` (`remove_dir_all`) then deleted the other's fixture
    /// mid-run.
    mod tempdir {
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        /// A scratch directory that is deleted, along with everything inside
        /// it, when the value is dropped.
        ///
        /// Create one with [`TempDir::new`] and pass [`path`](TempDir::path)
        /// to the code under test. Deletion errors are ignored, so a directory
        /// can survive on disk if the process aborts or is killed; bind the
        /// value to a named variable for the whole test, because dropping it
        /// (for example, through `let _ = ...`) deletes the directory
        /// immediately.
        pub struct TempDir {
            pub path: PathBuf,
        }

        impl TempDir {
            /// Creates a fresh directory under [`std::env::temp_dir`] and
            /// returns a [`TempDir`] that removes it on drop, so callers do
            /// not clean up.
            ///
            /// The directory name combines `prefix` with the process id, a
            /// nanosecond timestamp and a per-process counter, so two tests
            /// running in parallel cannot share one directory (see the module
            /// docs for why the timestamp alone is not a uniqueness
            /// guarantee). Pass a prefix that names the test that owns the
            /// directory, so a leftover one is traceable.
            ///
            /// # Panics
            ///
            /// Panics if the system clock is set before the Unix epoch, or if
            /// the directory cannot be created — for example, because of a
            /// permissions error or an exhausted filesystem.
            pub fn new(prefix: &str) -> Self {
                let mut path = std::env::temp_dir();
                let unique = format!(
                    "{prefix}-{}-{}-{}",
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
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }
}
