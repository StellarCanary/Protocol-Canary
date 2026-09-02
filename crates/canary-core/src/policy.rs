//! Compatibility policy types.
//!
//! [`Policy`] and [`PolicyDecision`] are defined here as part of the core
//! domain model; the evaluation rules that turn a set of
//! [`CompatibilityResult`](crate::model::CompatibilityResult)s into a
//! [`PolicyDecision`] are implemented by [`PolicyEvaluator`] and its
//! default implementation.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::errors::ExitCode;
use crate::model::{CompatibilityResult, Status};

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
///
/// This only looks at [`Status::Pass`], [`Status::Fail`], and
/// [`Status::Warning`]. [`Status::Error`] results are deliberately not a
/// policy input: an execution error (e.g. an RPC timeout) is not
/// compatibility evidence one way or the other, so it does not fail or
/// pass the *compatibility* decision. It still overrides the run's exit
/// code — see [`exit_code_for_run`] — because a run that could not
/// actually execute must not be reported as a clean pass.
pub trait PolicyEvaluator {
    fn evaluate(&self, results: &[CompatibilityResult], policy: &Policy) -> PolicyDecision;
}

/// The policy evaluator used in production.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPolicyEvaluator;

impl PolicyEvaluator for DefaultPolicyEvaluator {
    fn evaluate(&self, results: &[CompatibilityResult], policy: &Policy) -> PolicyDecision {
        let has_failure = results.iter().any(|r| r.status == Status::Fail);
        let has_warning = results.iter().any(|r| r.status == Status::Warning);

        if has_failure || (has_warning && policy.warnings_are_failures) {
            PolicyDecision::Fail
        } else if has_warning {
            PolicyDecision::Warning
        } else {
            PolicyDecision::Pass
        }
    }
}

/// Determines the process exit code for a completed run.
///
/// An execution error takes precedence over the compatibility decision:
/// if any result is [`Status::Error`], the run could not fully execute and
/// must exit [`ExitCode::ExecutionError`] regardless of what the other
/// results say. Otherwise the exit code follows the policy decision
/// directly (a `Warning` decision still exits 0 — it is not a failure
/// unless `Policy::warnings_are_failures` already promoted it to `Fail`
/// during evaluation).
pub fn exit_code_for_run(results: &[CompatibilityResult], decision: PolicyDecision) -> ExitCode {
    if results.iter().any(|r| r.status == Status::Error) {
        return ExitCode::ExecutionError;
    }
    match decision {
        PolicyDecision::Pass | PolicyDecision::Warning => ExitCode::Pass,
        PolicyDecision::Fail => ExitCode::CompatibilityFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProtocolVersion, Surface};

    fn result(status: Status) -> CompatibilityResult {
        CompatibilityResult {
            test_id: "t".into(),
            protocol: ProtocolVersion(28),
            surface: Surface::Xdr,
            status,
            summary: "s".into(),
            details: None,
            duration_ms: 0,
            fixture_id: None,
        }
    }

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

    #[test]
    fn all_passing_results_yield_a_pass_decision() {
        let results = vec![result(Status::Pass), result(Status::Pass)];
        let decision = DefaultPolicyEvaluator.evaluate(&results, &Policy::default());
        assert_eq!(decision, PolicyDecision::Pass);
    }

    #[test]
    fn a_warning_with_no_failures_yields_a_warning_decision() {
        let results = vec![result(Status::Pass), result(Status::Warning)];
        let decision = DefaultPolicyEvaluator.evaluate(&results, &Policy::default());
        assert_eq!(decision, PolicyDecision::Warning);
    }

    #[test]
    fn any_failure_yields_a_fail_decision() {
        let results = vec![result(Status::Pass), result(Status::Fail)];
        let decision = DefaultPolicyEvaluator.evaluate(&results, &Policy::default());
        assert_eq!(decision, PolicyDecision::Fail);
    }

    #[test]
    fn warnings_are_failures_promotes_a_warning_to_fail() {
        let results = vec![result(Status::Warning)];
        let policy = Policy {
            warnings_are_failures: true,
        };
        let decision = DefaultPolicyEvaluator.evaluate(&results, &policy);
        assert_eq!(decision, PolicyDecision::Fail);
    }

    #[test]
    fn execution_errors_do_not_affect_the_policy_decision() {
        let results = vec![result(Status::Pass), result(Status::Error)];
        let decision = DefaultPolicyEvaluator.evaluate(&results, &Policy::default());
        assert_eq!(decision, PolicyDecision::Pass);
    }

    #[test]
    fn execution_errors_override_the_exit_code_even_on_an_otherwise_passing_run() {
        let results = vec![result(Status::Pass), result(Status::Error)];
        let exit_code = exit_code_for_run(&results, PolicyDecision::Pass);
        assert_eq!(exit_code, ExitCode::ExecutionError);
    }

    #[test]
    fn a_pass_decision_with_no_errors_exits_zero() {
        let results = vec![result(Status::Pass)];
        let exit_code = exit_code_for_run(&results, PolicyDecision::Pass);
        assert_eq!(exit_code, ExitCode::Pass);
    }

    #[test]
    fn a_warning_decision_with_no_errors_still_exits_zero() {
        let results = vec![result(Status::Warning)];
        let exit_code = exit_code_for_run(&results, PolicyDecision::Warning);
        assert_eq!(exit_code, ExitCode::Pass);
    }

    #[test]
    fn a_fail_decision_exits_with_compatibility_failure() {
        let results = vec![result(Status::Fail)];
        let exit_code = exit_code_for_run(&results, PolicyDecision::Fail);
        assert_eq!(exit_code, ExitCode::CompatibilityFailure);
    }
}
