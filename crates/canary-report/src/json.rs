//! Machine-readable JSON output.
//!
//! Implemented alongside the versioned JSON schema in a later build step;
//! see `JsonReporter`.

/// Renders a [`crate::ReportInput`] as versioned JSON.
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonReporter;
