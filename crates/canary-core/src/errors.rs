//! Domain error taxonomy and process exit-code mapping.

use std::fmt;

/// Top-level error type returned by compatibility tests and the pipeline
/// stages that drive them.
///
/// Each variant corresponds to a pipeline stage rather than a specific
/// crate, so that `CanaryError` stays independent of the crates that
/// produce these errors (avoiding a dependency cycle back into
/// `canary-core`). Downstream crates define their own specific error enum
/// and convert it with `From`, e.g. `impl From<ConfigError> for CanaryError`.
#[derive(Debug, thiserror::Error)]
pub enum CanaryError {
    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("project detection error: {0}")]
    Project(String),

    #[error("invalid fixture: {0}")]
    Fixture(String),

    #[error("xdr error: {0}")]
    Xdr(String),

    #[error("rpc error: {0}")]
    Rpc(String),

    #[error("soroban error: {0}")]
    Soroban(String),

    #[error("git error: {0}")]
    Git(String),

    #[error("cache error: {0}")]
    Cache(String),

    #[error("internal error: {0}")]
    Internal(String),
}

/// The category of a [`CanaryError`], used to decide the process exit code
/// without matching on error message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Configuration,
    Fixture,
    Execution,
    Internal,
}

impl CanaryError {
    /// Classifies this error for exit-code mapping purposes.
    pub fn category(&self) -> ErrorCategory {
        match self {
            CanaryError::Configuration(_) | CanaryError::Project(_) => ErrorCategory::Configuration,
            CanaryError::Fixture(_) => ErrorCategory::Fixture,
            CanaryError::Xdr(_) | CanaryError::Rpc(_) | CanaryError::Soroban(_) => {
                ErrorCategory::Execution
            }
            CanaryError::Git(_) | CanaryError::Cache(_) | CanaryError::Internal(_) => {
                ErrorCategory::Internal
            }
        }
    }
}

/// Process exit codes for the `stellar-canary` CLI.
///
/// These are a stable contract: GitHub automation and other callers must be
/// able to rely on them rather than parsing terminal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Pass = 0,
    CompatibilityFailure = 1,
    ConfigurationError = 2,
    ExecutionError = 3,
    InvalidFixture = 4,
    InternalError = 5,
}

impl ExitCode {
    /// Every exit code, in ascending numeric order.
    ///
    /// This is the shared definition the documentation is checked
    /// against: `docs-site/src/exit-codes.md` must document exactly this
    /// set, which `canary-cli`'s `exit_code_docs` integration tests
    /// enforce by parsing that page. The exhaustive match in this
    /// module's tests fails to compile if a variant is added without
    /// being listed here, so a new code can never stay undocumented.
    pub const ALL: &[ExitCode] = &[
        ExitCode::Pass,
        ExitCode::CompatibilityFailure,
        ExitCode::ConfigurationError,
        ExitCode::ExecutionError,
        ExitCode::InvalidFixture,
        ExitCode::InternalError,
    ];

    pub fn code(self) -> i32 {
        self as i32
    }
}

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ExitCode::Pass => "pass",
            ExitCode::CompatibilityFailure => "compatibility_failure",
            ExitCode::ConfigurationError => "configuration_error",
            ExitCode::ExecutionError => "execution_error",
            ExitCode::InvalidFixture => "invalid_fixture",
            ExitCode::InternalError => "internal_error",
        };
        f.write_str(name)
    }
}

impl From<&CanaryError> for ExitCode {
    fn from(error: &CanaryError) -> Self {
        match error.category() {
            ErrorCategory::Configuration => ExitCode::ConfigurationError,
            ErrorCategory::Fixture => ExitCode::InvalidFixture,
            ErrorCategory::Execution => ExitCode::ExecutionError,
            ErrorCategory::Internal => ExitCode::InternalError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_and_project_errors_map_to_configuration_error_exit_code() {
        let config_err = CanaryError::Configuration("bad toml".into());
        let project_err = CanaryError::Project("ambiguous project type".into());
        assert_eq!(ExitCode::from(&config_err), ExitCode::ConfigurationError);
        assert_eq!(ExitCode::from(&project_err), ExitCode::ConfigurationError);
    }

    #[test]
    fn fixture_errors_map_to_invalid_fixture_exit_code() {
        let err = CanaryError::Fixture("duplicate fixture id".into());
        assert_eq!(ExitCode::from(&err), ExitCode::InvalidFixture);
    }

    #[test]
    fn surface_execution_errors_map_to_execution_error_exit_code() {
        for err in [
            CanaryError::Xdr("decode failure".into()),
            CanaryError::Rpc("timeout".into()),
            CanaryError::Soroban("simulation failed".into()),
        ] {
            assert_eq!(ExitCode::from(&err), ExitCode::ExecutionError);
        }
    }

    #[test]
    fn git_cache_and_internal_errors_map_to_internal_error_exit_code() {
        for err in [
            CanaryError::Git("not a repository".into()),
            CanaryError::Cache("corrupt cache entry".into()),
            CanaryError::Internal("unreachable state".into()),
        ] {
            assert_eq!(ExitCode::from(&err), ExitCode::InternalError);
        }
    }

    #[test]
    fn all_lists_every_exit_code_variant() {
        // Exhaustive on purpose: adding an `ExitCode` variant without
        // also listing it in `ExitCode::ALL` fails to compile here,
        // which in turn forces the documentation consistency tests in
        // `canary-cli` to see (and therefore require) the new code.
        for &code in ExitCode::ALL {
            match code {
                ExitCode::Pass
                | ExitCode::CompatibilityFailure
                | ExitCode::ConfigurationError
                | ExitCode::ExecutionError
                | ExitCode::InvalidFixture
                | ExitCode::InternalError => {}
            }
        }
    }

    #[test]
    fn exit_code_all_is_ascending_and_duplicate_free() {
        let codes: Vec<i32> = ExitCode::ALL.iter().map(|c| c.code()).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes, sorted, "ExitCode::ALL must be in ascending order");
        assert_eq!(
            codes.len(),
            ExitCode::ALL.len(),
            "ExitCode::ALL must not repeat a code"
        );
    }

    #[test]
    fn exit_codes_match_the_documented_contract() {
        assert_eq!(ExitCode::Pass.code(), 0);
        assert_eq!(ExitCode::CompatibilityFailure.code(), 1);
        assert_eq!(ExitCode::ConfigurationError.code(), 2);
        assert_eq!(ExitCode::ExecutionError.code(), 3);
        assert_eq!(ExitCode::InvalidFixture.code(), 4);
        assert_eq!(ExitCode::InternalError.code(), 5);
    }

    #[test]
    fn exit_codes_display_as_their_documented_lowercase_names() {
        assert_eq!(ExitCode::Pass.to_string(), "pass");
        assert_eq!(
            ExitCode::CompatibilityFailure.to_string(),
            "compatibility_failure"
        );
        assert_eq!(
            ExitCode::ConfigurationError.to_string(),
            "configuration_error"
        );
        assert_eq!(ExitCode::ExecutionError.to_string(), "execution_error");
        assert_eq!(ExitCode::InvalidFixture.to_string(), "invalid_fixture");
        assert_eq!(ExitCode::InternalError.to_string(), "internal_error");
    }
}
