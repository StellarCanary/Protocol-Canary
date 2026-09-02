//! Compatibility test scheduling, execution, and aggregation for Stellar
//! Protocol Canary.

pub mod aggregation;
pub mod execution;
pub mod scheduler;

pub use aggregation::{summarize, ResultSummary};
pub use execution::execute;
pub use scheduler::{build_plan, CompatibilityPlan, EnabledSurfaces, SkippedFixture};
