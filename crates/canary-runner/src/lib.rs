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

/// Whether fixture `id` matches `pattern`. `*` matches any run of
/// characters, including none; every other character matches itself, so a
/// pattern without `*` is an exact match.
pub fn fixture_id_matches(pattern: &str, id: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == id;
    }
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = id.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return false;
    };
    for &part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// Selects fixtures whose ID matches the optional pattern (see
/// [`fixture_id_matches`]) and whose surface matches the optional surface.
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
            fixture_id.is_none_or(|pattern| fixture_id_matches(pattern, &fixture.metadata.id))
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

    #[test]
    fn a_wildcard_selects_every_matching_id_in_order() {
        let fixtures = vec![
            fixture("contract_b", "xdr"),
            fixture("other", "xdr"),
            fixture("contract_a", "xdr"),
        ];
        let ids: Vec<&str> = filter_fixtures(&fixtures, Some("contract_*"), None)
            .iter()
            .map(|f| f.metadata.id.as_str())
            .collect();
        assert_eq!(ids, ["contract_b", "contract_a"]);
        assert!(filter_fixtures(&fixtures, Some("nothing_*"), None).is_empty());
    }

    #[test]
    fn wildcard_matching_follows_glob_star_rules() {
        assert!(fixture_id_matches("abc", "abc"));
        assert!(!fixture_id_matches("abc", "abcd"));
        assert!(fixture_id_matches("*", ""));
        assert!(fixture_id_matches("*", "anything"));
        assert!(fixture_id_matches("p28-*", "p28-xdr-1"));
        assert!(!fixture_id_matches("p28-*", "p27-xdr-1"));
        assert!(fixture_id_matches("*-1", "p28-xdr-1"));
        assert!(fixture_id_matches("p28-*-1", "p28-xdr-1"));
        assert!(fixture_id_matches("p*x*1", "p28-xdr-1"));
        assert!(!fixture_id_matches("a*a", "a"));
        assert!(fixture_id_matches("a**b", "ab"));
        assert!(!fixture_id_matches("p28-*-2", "p28-xdr-1"));
    }
}
