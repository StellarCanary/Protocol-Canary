//! Deciding which loaded fixtures apply to a run, and why others don't.

use canary_core::{CanaryError, Capability, ProjectContext, ProtocolVersion, SkipCode, Surface};
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

/// Whether the run may touch the network.
///
/// XDR fixtures are always offline; RPC and Soroban fixtures always need a
/// live endpoint. [`NetworkMode::Offline`] (the `stellar-canary check
/// --offline` flag) makes the planner skip the network-requiring fixtures
/// instead of scheduling them, so an air-gapped run executes only the
/// offline XDR checks and reports the rest as skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkMode {
    /// Network-requiring fixtures execute against the configured endpoint.
    #[default]
    Live,
    /// Network-requiring fixtures are skipped, never executed.
    Offline,
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
/// Why a loaded fixture was left out of the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipCause {
    /// The fixture targets a different protocol than the run.
    ProtocolMismatch,
    /// The fixture's surface is switched off in configuration.
    SurfaceDisabled,
    /// The project lacks a capability the fixture requires.
    MissingCapability,
    /// The run is offline (`--offline`) and the fixture's surface requires
    /// network access (RPC, Soroban).
    Offline,
}

impl SkipCause {
    /// The stable machine-readable reason this fixture was skipped,
    /// carried into the report as `skipped[].code` (CF-01).
    ///
    /// The free-form `reason` stays prose; this is what tooling keys off.
    pub fn code(self) -> SkipCode {
        match self {
            SkipCause::ProtocolMismatch => SkipCode::UnsupportedProtocol,
            SkipCause::SurfaceDisabled => SkipCode::SurfaceDisabled,
            SkipCause::MissingCapability => SkipCode::MissingCapability,
            SkipCause::Offline => SkipCode::Offline,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFixture {
    pub fixture_id: String,
    pub surface: Surface,
    pub reason: String,
    pub cause: SkipCause,
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
    /// Returns the total number of fixtures scheduled to run across all enabled surfaces.
    ///
    /// This is the sum of the scheduled XDR, RPC, and Soroban fixtures. It does not
    /// include fixtures that were skipped.
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
    build_plan_with(
        loaded_fixtures,
        target_protocol,
        enabled_surfaces,
        project,
        NetworkMode::Live,
    )
}

/// [`build_plan`] with an explicit [`NetworkMode`].
///
/// Under [`NetworkMode::Offline`], fixtures whose surface requires network
/// access (RPC and Soroban) are skipped with
/// [`SkipCause::Offline`] instead of being scheduled, after the protocol,
/// surface-enabled, and capability checks — so a fixture that would be
/// skipped for another reason still reports that reason, and the offline
/// skip only applies to fixtures that would otherwise run.
pub fn build_plan_with(
    loaded_fixtures: &[LoadedFixture],
    target_protocol: ProtocolVersion,
    enabled_surfaces: EnabledSurfaces,
    project: &ProjectContext,
    network_mode: NetworkMode,
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
                cause: SkipCause::ProtocolMismatch,
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
                cause: SkipCause::SurfaceDisabled,
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
                cause: SkipCause::MissingCapability,
            });
            continue;
        }

        // Every non-XDR surface talks to a live RPC endpoint, so an
        // offline run skips them rather than opening a socket. This is
        // checked last so a fixture that is inapplicable anyway keeps its
        // more specific reason.
        if network_mode == NetworkMode::Offline && fixture.metadata.surface != Surface::Xdr {
            plan.skipped.push(SkippedFixture {
                fixture_id: fixture.metadata.id.clone(),
                surface: fixture.metadata.surface,
                reason: "requires network access; skipped because --offline was given".to_string(),
                cause: SkipCause::Offline,
            });
            continue;
        }

        match fixture.metadata.surface {
            // A body the surface cannot read is a problem with the fixture,
            // not an execution error: it is exit 4 like any other invalid
            // fixture (`docs/fixture-contract.md`), not exit 3.
            Surface::Xdr => plan.xdr.push(
                XdrFixture::from_loaded(fixture)
                    .map_err(|e| CanaryError::Fixture(e.to_string()))?,
            ),
            Surface::Rpc => plan.rpc.push(
                RpcFixture::from_loaded(fixture)
                    .map_err(|e| CanaryError::Fixture(e.to_string()))?,
            ),
            Surface::Soroban => plan.soroban.push(
                SorobanFixture::from_loaded(fixture)
                    .map_err(|e| CanaryError::Fixture(e.to_string()))?,
            ),
        }
    }

    Ok(plan)
}

/// Explains, in one message, why a plan contains nothing to run.
///
/// A run that executes no fixture proves nothing about compatibility, so
/// `check` refuses it unless the caller opts in. This names which of the
/// three possible reasons applies (no fixtures were loaded, every loaded
/// fixture was filtered out, or the project lacks a required capability) so
/// the fix is obvious. The text does not mention `--allow-empty`; the caller
/// adds that where it applies. Call it only when
/// [`CompatibilityPlan::applicable_count`] is zero.
pub fn explain_empty_plan(
    plan: &CompatibilityPlan,
    loaded_fixtures: &[LoadedFixture],
    target_protocol: ProtocolVersion,
    fixtures_dir: &std::path::Path,
) -> String {
    let mut message = String::from("no checks ran: ");

    if loaded_fixtures.is_empty() {
        if fixtures_dir.is_dir() {
            message.push_str(&format!(
                "no fixture files (*.toml) were found in {}.",
                fixtures_dir.display()
            ));
        } else {
            message.push_str(&format!(
                "the fixtures directory {} does not exist.",
                fixtures_dir.display()
            ));
        }
        message.push_str(" Point --fixtures-dir at a directory of fixtures.");
    } else {
        message.push_str(&format!(
            "all {} loaded fixtures were skipped.",
            loaded_fixtures.len()
        ));
        let count = |cause: SkipCause| plan.skipped.iter().filter(|s| s.cause == cause).count();

        let mismatched = count(SkipCause::ProtocolMismatch);
        if mismatched > 0 {
            let mut protocols: Vec<u32> = loaded_fixtures
                .iter()
                .map(|f| f.metadata.protocol.0)
                .filter(|p| *p != target_protocol.0)
                .collect();
            protocols.sort_unstable();
            protocols.dedup();
            let protocols: Vec<String> = protocols.iter().map(u32::to_string).collect();
            message.push_str(&format!(
                " {mismatched} target another protocol (this run targets protocol {target_protocol}; the fixtures target protocol {}).",
                protocols.join(", ")
            ));
        }
        let disabled = count(SkipCause::SurfaceDisabled);
        if disabled > 0 {
            message.push_str(&format!(
                " {disabled} are on a surface that is disabled in configuration."
            ));
        }
        let missing = count(SkipCause::MissingCapability);
        if missing > 0 {
            message.push_str(&format!(
                " {missing} require a capability this project does not declare."
            ));
        }
        let offline = count(SkipCause::Offline);
        if offline > 0 {
            message.push_str(&format!(
                " {offline} require network access and were skipped because --offline was given."
            ));
        }
    }

    message
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
    fn schedules_a_matching_rpc_fixture() {
        let fixtures = vec![loaded("p28-rpc-1", 28, "rpc", "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n")];
        let plan = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project()).unwrap();
        assert_eq!(plan.rpc.len(), 1);
        assert_eq!(plan.rpc[0].metadata.id, "p28-rpc-1");
        assert!(plan.xdr.is_empty());
        assert!(plan.soroban.is_empty());
        assert!(plan.skipped.is_empty());
        assert_eq!(plan.applicable_count(), 1);
    }

    #[test]
    fn schedules_a_matching_soroban_fixture() {
        let body = format!(
            "source_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"hello\"\nsequence_number = 1\n\n[expect]\nkind = \"simulation-success\"\n",
            stellar_strkey::ed25519::PublicKey([0u8; 32]),
            stellar_strkey::Contract([0u8; 32]),
        );
        let fixtures = vec![loaded("p28-soroban-1", 28, "soroban", &body)];
        let plan = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project()).unwrap();
        assert_eq!(plan.soroban.len(), 1);
        assert_eq!(plan.soroban[0].metadata.id, "p28-soroban-1");
        assert!(plan.xdr.is_empty());
        assert!(plan.rpc.is_empty());
        assert!(plan.skipped.is_empty());
        assert_eq!(plan.applicable_count(), 1);
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
            loaded("p28-rpc-1", 28, "rpc", "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n"),
        ];
        let plan = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project()).unwrap();
        assert_eq!(plan.applicable_count(), 2);
    }

    const XDR_BODY: &str =
        "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n";

    #[test]
    fn every_skip_records_a_stable_cause() {
        let fixtures = vec![
            loaded("a-p27", 27, "xdr", XDR_BODY),
            loaded("b-disabled", 28, "xdr", XDR_BODY),
        ];
        let off = EnabledSurfaces {
            xdr: false,
            rpc: true,
            soroban: true,
        };
        let plan = build_plan(&fixtures, ProtocolVersion(28), off, &project()).unwrap();
        let codes: Vec<_> = plan.skipped.iter().map(|s| s.cause.code()).collect();
        assert_eq!(
            codes,
            [SkipCode::UnsupportedProtocol, SkipCode::SurfaceDisabled]
        );
        assert_eq!(
            SkipCause::MissingCapability.code(),
            SkipCode::MissingCapability
        );
    }

    #[test]
    fn every_skip_cause_maps_to_a_standard_report_code() {
        // The causes the planner can produce; NETWORK_MISMATCH and
        // UNKNOWN are reserved/derived and never produced by a cause.
        assert_eq!(
            SkipCause::ProtocolMismatch.code().as_str(),
            "UNSUPPORTED_PROTOCOL"
        );
        assert_eq!(
            SkipCause::SurfaceDisabled.code().as_str(),
            "SURFACE_DISABLED"
        );
        assert_eq!(
            SkipCause::MissingCapability.code().as_str(),
            "MISSING_CAPABILITY"
        );
    }

    #[test]
    fn offline_mode_schedules_xdr_and_skips_rpc_and_soroban() {
        let fixtures = vec![
            loaded(
                "p28-xdr-1",
                28,
                "xdr",
                "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n",
            ),
            loaded("p28-rpc-1", 28, "rpc", "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n"),
            loaded("p28-soroban-1", 28, "soroban", &soroban_body()),
        ];
        let plan = build_plan_with(
            &fixtures,
            ProtocolVersion(28),
            all_enabled(),
            &project(),
            NetworkMode::Offline,
        )
        .unwrap();

        assert_eq!(plan.applicable_count(), 1, "only the XDR fixture runs");
        assert_eq!(plan.xdr.len(), 1);
        assert!(plan.rpc.is_empty());
        assert!(plan.soroban.is_empty());
        assert_eq!(plan.skipped.len(), 2);
        for skipped in &plan.skipped {
            assert_eq!(skipped.cause, SkipCause::Offline);
            assert!(skipped.reason.contains("--offline"), "{}", skipped.reason);
        }
    }

    #[test]
    fn the_offline_skip_cause_has_a_stable_code() {
        assert_eq!(SkipCause::Offline.code(), SkipCode::Offline);
    }

    /// A fixture that would be skipped for a more specific reason keeps it:
    /// the offline skip only applies to fixtures that would otherwise run.
    #[test]
    fn protocol_mismatch_takes_precedence_over_the_offline_skip() {
        let fixtures = vec![loaded(
            "p27-rpc-1",
            27,
            "rpc",
            "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n",
        )];
        let plan = build_plan_with(
            &fixtures,
            ProtocolVersion(28),
            all_enabled(),
            &project(),
            NetworkMode::Offline,
        )
        .unwrap();

        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].cause, SkipCause::ProtocolMismatch);
    }

    #[test]
    fn live_mode_still_schedules_rpc_fixtures() {
        // The default `build_plan` is Live: offline skipping is opt-in.
        let fixtures = vec![loaded(
            "p28-rpc-1",
            28,
            "rpc",
            "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n",
        )];
        let plan = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project()).unwrap();
        assert_eq!(plan.applicable_count(), 1);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn explains_an_empty_offline_plan_and_names_the_flag() {
        let fixtures = vec![
            loaded(
                "p28-xdr-1",
                28,
                "xdr",
                "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n",
            ),
            loaded("p28-rpc-1", 28, "rpc", "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n"),
        ];
        // XDR disabled, so under offline mode everything is skipped.
        let off = EnabledSurfaces {
            xdr: false,
            rpc: true,
            soroban: true,
        };
        let plan = build_plan_with(
            &fixtures,
            ProtocolVersion(28),
            off,
            &project(),
            NetworkMode::Offline,
        )
        .unwrap();
        assert_eq!(plan.applicable_count(), 0);
        let message = explain_empty_plan(
            &plan,
            &fixtures,
            ProtocolVersion(28),
            std::path::Path::new("."),
        );
        assert!(
            message
                .contains("1 require network access and were skipped because --offline was given"),
            "{message}"
        );
        assert!(
            message.contains("1 are on a surface that is disabled"),
            "{message}"
        );
    }

    fn soroban_body() -> String {
        format!(
            "source_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"hello\"\nsequence_number = 1\n\n[expect]\nkind = \"simulation-success\"\n",
            stellar_strkey::ed25519::PublicKey([0u8; 32]),
            stellar_strkey::Contract([0u8; 32]),
        )
    }

    #[test]
    fn explains_a_missing_fixtures_directory() {
        let plan = CompatibilityPlan::default();
        let message = explain_empty_plan(
            &plan,
            &[],
            ProtocolVersion(28),
            std::path::Path::new("no-such-fixtures-dir-for-this-test"),
        );
        assert!(message.contains("does not exist"), "{message}");
        assert!(message.contains("--fixtures-dir"), "{message}");
    }

    #[test]
    fn explains_a_directory_with_no_fixture_files() {
        let plan = CompatibilityPlan::default();
        let message = explain_empty_plan(&plan, &[], ProtocolVersion(28), &std::env::temp_dir());
        assert!(
            message.contains("no fixture files (*.toml) were found"),
            "{message}"
        );
    }

    #[test]
    fn explains_fixtures_for_another_protocol_and_names_both_protocols() {
        let fixtures = vec![
            loaded("a", 28, "xdr", XDR_BODY),
            loaded("b", 28, "xdr", XDR_BODY),
            loaded("c", 27, "xdr", XDR_BODY),
        ];
        let plan = build_plan(&fixtures, ProtocolVersion(29), all_enabled(), &project()).unwrap();
        assert_eq!(plan.applicable_count(), 0);
        let message = explain_empty_plan(
            &plan,
            &fixtures,
            ProtocolVersion(29),
            std::path::Path::new("."),
        );
        assert!(
            message.contains("all 3 loaded fixtures were skipped"),
            "{message}"
        );
        assert!(message.contains("3 target another protocol"), "{message}");
        assert!(
            message.contains("this run targets protocol 29"),
            "{message}"
        );
        assert!(
            message.contains("fixtures target protocol 27, 28"),
            "{message}"
        );
    }

    #[test]
    fn explains_a_disabled_surface_and_a_missing_capability_together() {
        let soroban = format!(
            "id = \"s\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"c\"\ndescription = \"d\"\nrequired_capabilities = [\"soroban-contract\"]\nsource_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"f\"\nsequence_number = 1\n\n[expect]\nkind = \"simulation-success\"\n",
            stellar_strkey::ed25519::PublicKey([0u8; 32]),
            stellar_strkey::Contract([0u8; 32]),
        );
        let fixtures = vec![
            loaded("x", 28, "xdr", XDR_BODY),
            canary_fixtures::parse_fixture_str(&soroban, std::path::Path::new("s.toml")).unwrap(),
        ];
        let off = EnabledSurfaces {
            xdr: false,
            rpc: true,
            soroban: true,
        };
        let plan = build_plan(&fixtures, ProtocolVersion(28), off, &project()).unwrap();
        let message = explain_empty_plan(
            &plan,
            &fixtures,
            ProtocolVersion(28),
            std::path::Path::new("."),
        );
        assert!(
            message.contains("1 are on a surface that is disabled"),
            "{message}"
        );
        assert!(
            message.contains("1 require a capability this project does not declare"),
            "{message}"
        );
        assert!(!message.contains("another protocol"), "{message}");
    }
}
