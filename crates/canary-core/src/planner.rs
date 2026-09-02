//! Compatibility planning: deciding which fixtures apply to a project and
//! why others are skipped.
//!
//! The full planner (fixture applicability, skip reasons, deterministic
//! ordering) is implemented alongside the execution engine; see
//! `CompatibilityPlanner` in the runner build stage.

/// Decides which fixtures apply to a given project and protocol.
///
/// This is currently a marker type; [`crate::engine`] and the fixture/runner
/// crates provide the data it plans over, added in a later stage of the
/// build.
#[derive(Debug, Default, Clone, Copy)]
pub struct CompatibilityPlanner;
