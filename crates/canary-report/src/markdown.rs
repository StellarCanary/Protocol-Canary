//! GitHub-friendly Markdown output.
//!
//! Implemented in a later build step; see `MarkdownReporter`.

/// Renders a [`crate::ReportInput`] as Markdown suitable for a GitHub job
/// summary.
#[derive(Debug, Default, Clone, Copy)]
pub struct MarkdownReporter;
