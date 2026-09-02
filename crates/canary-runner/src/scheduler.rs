//! Deciding which loaded fixtures apply to a run, and why others don't.

use canary_core::{CanaryError, Capability, ProjectContext, ProtocolVersion, Surface};
use canary_fixtures::LoadedFixture;
use canary_rpc::RpcFixture;
use canary_soroban::SorobanFixture;
use canary_xdr::XdrFixture;

/// Which surfaces are enabled for this run (from configuration).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnabledSurfaces {
    pub xdr: bool,
    pub rpc: bool,
    pub soroban: bool,
}

impl EnabledSurfaces {
    fn is_enabled(self, surface: Surface) -> bool {
        match surface {
            Surface::Xdr => self.xdr,
            Surface::Rpc => self.rpc,
            Surface::Soroban => self.soroban,
        }
    }
}

/// A fixture that was not scheduled to run, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFixture {
    pub fixture_id: String,
    pub surface: Surface,
    pub reason: String,
}

/// The result of planning: which fixtures will run, grouped by surface (so
/// that terminal/JSON/Markdown reports can present them grouped exactly
/// this way), and which were skipped and why.
#[derive(Debug, Default)]
pub struct CompatibilityPlan {
    pub xdr: Vec<XdrFixture>,
    pub rpc: Vec<RpcFixture>,
    pub soroban: Vec<SorobanFixture>,
    pub skipped: Vec<SkippedFixture>,
}

impl CompatibilityPlan {
    pub fn applicable_count(&self) -> usize {
        self.xdr.len() + self.rpc.len() + self.soroban.len()
    }
}

/// Builds a [`CompatibilityPlan`] from a set of loaded fixtures.
///
/// `loaded_fixtures` must already be in the deterministic order produced by
/// [`canary_fixtures::load_directory`]; that order is preserved into the
/// plan's per-surface vectors.
pub fn build_plan(
    loaded_fixtures: &[LoadedFixture],
    target_protocol: ProtocolVersion,
    enabled_surfaces: EnabledSurfaces,
    project: &ProjectContext,
) -> Result<CompatibilityPlan, CanaryError> {
    let mut plan = CompatibilityPlan::default();

    for fixture in loaded_fixtures {
        if fixture.metadata.protocol != target_protocol {
            plan.skipped.push(SkippedFixture {
                fixture_id: fixture.metadata.id.clone(),
                surface: fixture.metadata.surface,
                reason: format!(
                    "fixture targets protocol {}, this run targets protocol {}",
                    fixture.metadata.protocol, target_protocol
                ),
            });
            continue;
        }

        if !enabled_surfaces.is_enabled(fixture.metadata.surface) {
            plan.skipped.push(SkippedFixture {
                fixture_id: fixture.metadata.id.clone(),
                surface: fixture.metadata.surface,
                reason: format!(
                    "{} checks are disabled in configuration",
                    fixture.metadata.surface
                ),
            });
            continue;
        }

        let missing: Vec<&Capability> = fixture
            .metadata
            .required_capabilities
            .iter()
            .filter(|cap| !project.has_capability(cap))
            .collect();
        if !missing.is_empty() {
            plan.skipped.push(SkippedFixture {
                fixture_id: fixture.metadata.id.clone(),
                surface: fixture.metadata.surface,
                reason: format!("requires a capability not declared by this project: {missing:?}"),
            });
            continue;
        }

        match fixture.metadata.surface {
            Surface::Xdr => plan.xdr.push(XdrFixture::from_loaded(fixture)?),
            Surface::Rpc => plan.rpc.push(RpcFixture::from_loaded(fixture)?),
            Surface::Soroban => plan.soroban.push(SorobanFixture::from_loaded(fixture)?),
        }
    }

    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canary_core::ProjectType;

    fn loaded(id: &str, protocol: u32, surface: &str, body: &str) -> LoadedFixture {
        canary_fixtures::parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = {protocol}\nsurface = \"{surface}\"\ncategory = \"c\"\ndescription = \"d\"\n{body}"
            ),
            std::path::Path::new("test.toml"),
        )
        .unwrap()
    }

    fn project() -> ProjectContext {
        ProjectContext {
            root: ".".into(),
            name: "test".into(),
            project_type: ProjectType::Unknown,
            capabilities: vec![],
        }
    }

    fn all_enabled() -> EnabledSurfaces {
        EnabledSurfaces {
            xdr: true,
            rpc: true,
            soroban: true,
        }
    }

    #[test]
    fn schedules_a_matching_xdr_fixture() {
        let fixtures = vec![loaded(
            "p28-xdr-1",
            28,
            "xdr",
            "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n",
        )];
        let plan = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project()).unwrap();
        assert_eq!(plan.xdr.len(), 1);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn skips_a_fixture_targeting_a_different_protocol() {
        let fixtures = vec![loaded(
            "p27-xdr-1",
            27,
            "xdr",
            "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n",
        )];
        let plan = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project()).unwrap();
        assert_eq!(plan.xdr.len(), 0);
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].reason.contains("protocol 27"));
    }

    #[test]
    fn skips_a_fixture_for_a_disabled_surface() {
        let fixtures = vec![loaded(
            "p28-xdr-1",
            28,
            "xdr",
            "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n",
        )];
        let disabled_xdr = EnabledSurfaces {
            xdr: false,
            rpc: true,
            soroban: true,
        };
        let plan = build_plan(&fixtures, ProtocolVersion(28), disabled_xdr, &project()).unwrap();
        assert_eq!(plan.xdr.len(), 0);
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].reason.contains("disabled"));
    }

    #[test]
    fn skips_a_fixture_requiring_an_undeclared_capability() {
        let toml = format!(
            "id = \"p28-soroban-1\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"c\"\ndescription = \"d\"\nrequired_capabilities = [\"soroban-contract\"]\nsource_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"f\"\nsequence_number = 1\n\n[expect]\nkind = \"simulation-success\"\n",
            stellar_strkey::ed25519::PublicKey([0u8; 32]),
            stellar_strkey::Contract([0u8; 32]),
        );
        let fixtures =
            vec![
                canary_fixtures::parse_fixture_str(&toml, std::path::Path::new("t.toml")).unwrap(),
            ];
        let plan = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project()).unwrap();
        assert_eq!(plan.soroban.len(), 0);
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].reason.contains("capability"));
    }

    #[test]
    fn applicable_count_sums_all_surfaces() {
        let fixtures = vec![
            loaded(
                "p28-xdr-1",
                28,
                "xdr",
                "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n",
            ),
            loaded("p28-rpc-1", 28, "rpc", "method = \"get-network\"\n"),
        ];
        let plan = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project()).unwrap();
        assert_eq!(plan.applicable_count(), 2);
    }
}
