//! Compatibility test scheduling, execution, and aggregation for Stellar
//! Protocol Canary.

use canary_core::Surface;
use canary_fixtures::LoadedFixture;

pub mod aggregation;
pub mod execution;
pub mod scheduler;

pub use aggregation::{summarize, ResultSummary};
pub use execution::execute;
pub use scheduler::{
    build_plan, build_plan_with, explain_empty_plan, CompatibilityPlan, EnabledSurfaces,
    NetworkMode, SkipCause, SkippedFixture,
};

/// Selects fixtures matching the optional exact fixture ID and surface.
///
/// The input ordering is preserved, and an empty result means no fixture
/// matched. Callers can then report that condition with command-specific
/// context rather than attempting an empty run.
pub fn filter_fixtures<'a>(
    fixtures: &'a [LoadedFixture],
    fixture_id: Option<&str>,
    surface: Option<Surface>,
) -> Vec<&'a LoadedFixture> {
    fixtures
        .iter()
        .filter(|fixture| {
            fixture_id.is_none_or(|id| fixture.metadata.id == id)
                && surface.is_none_or(|selected| fixture.metadata.surface == selected)
        })
        .collect()
}

#[cfg(test)]
mod filter_tests {
    use super::*;
    use std::path::Path;

    fn fixture(id: &str, surface: &str) -> LoadedFixture {
        canary_fixtures::parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"{surface}\"\ncategory = \"test\"\ndescription = \"test\"\n"
            ),
            Path::new("fixtures/test.toml"),
        )
        .unwrap()
    }

    #[test]
    fn filters_by_exact_fixture_id_and_preserves_order() {
        let fixtures = vec![fixture("xdr-a", "xdr"), fixture("xdr-b", "xdr")];
        let selected = filter_fixtures(&fixtures, Some("xdr-b"), None);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].metadata.id, "xdr-b");
    }

    #[test]
    fn filters_by_surface() {
        let fixtures = vec![fixture("xdr-a", "xdr"), fixture("rpc-a", "rpc")];
        let selected = filter_fixtures(&fixtures, None, Some(Surface::Rpc));
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].metadata.id, "rpc-a");
    }

    #[test]
    fn returns_no_fixtures_for_a_nonmatching_filter() {
        let fixtures = vec![fixture("xdr-a", "xdr")];
        assert!(filter_fixtures(&fixtures, Some("missing"), None).is_empty());
    }
}
