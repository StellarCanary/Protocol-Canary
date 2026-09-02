//! Loading and validating `.stellar-canary.toml`.

use std::path::{Path, PathBuf};

use canary_core::CanaryError;

use crate::schema::{ConfigFile, SUPPORTED_CONFIG_VERSION};

/// The default configuration file name looked for in a project root.
pub const CONFIG_FILE_NAME: &str = ".stellar-canary.toml";

#[derive(Debug, thiserror::Error)]
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
    mod tempdir {
        use std::path::PathBuf;

        pub struct TempDir {
            pub path: PathBuf,
        }

        impl TempDir {
            pub fn new(prefix: &str) -> Self {
                let mut path = std::env::temp_dir();
                let unique = format!(
                    "{prefix}-{}-{}",
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
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }
}
