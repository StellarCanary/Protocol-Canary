//! Compatibility policy types.
//!
//! [`Policy`] and [`PolicyDecision`] are defined here as part of the core
//! domain model; the evaluation rules that turn a set of
//! [`CompatibilityResult`](crate::model::CompatibilityResult)s into a
//! [`PolicyDecision`] are implemented by [`PolicyEvaluator`] and its
//! default implementation.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::CompatibilityResult;

/// User-configurable policy for turning results into a pass/warn/fail
/// decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Policy {
    pub warnings_are_failures: bool,
}

/// The overall outcome of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyDecision {
    Pass,
    Warning,
    Fail,
}

impl fmt::Display for PolicyDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            PolicyDecision::Pass => "pass",
            PolicyDecision::Warning => "warning",
            PolicyDecision::Fail => "fail",
        };
        f.write_str(name)
    }
}

/// Turns a set of results into a [`PolicyDecision`] under a [`Policy`].
pub trait PolicyEvaluator {
    fn evaluate(&self, results: &[CompatibilityResult], policy: &Policy) -> PolicyDecision;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_does_not_promote_warnings_to_failures() {
        assert!(!Policy::default().warnings_are_failures);
    }

    #[test]
    fn policy_decision_displays_lowercase() {
        assert_eq!(PolicyDecision::Pass.to_string(), "pass");
        assert_eq!(PolicyDecision::Warning.to_string(), "warning");
        assert_eq!(PolicyDecision::Fail.to_string(), "fail");
    }
}
