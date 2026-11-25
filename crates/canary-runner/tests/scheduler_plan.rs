//! Multi-surface scheduling and offline network mode transitions (issue #820).
//!
//! The scheduler partitions loaded fixtures into per-surface plans and
//! records a [`SkipCause`] for every fixture it leaves out. These tests pin
//! the two behaviours that matter to an air-gapped run:
//!
//! - [`NetworkMode::Live`] schedules XDR, RPC and Soroban fixtures, while
//!   [`NetworkMode::Offline`] keeps the synchronous XDR checks and routes the
//!   network-bound RPC and Soroban fixtures to the skipped collection with
//!   [`SkipCause::Offline`].
//! - [`EnabledSurfaces`] toggles each surface independently without panicking
//!   or producing an inconsistent plan, and capability filtering keeps only
//!   the fixtures the project context can run.

use canary_core::{Capability, ProjectContext, ProjectType, ProtocolVersion, SkipCode, Surface};
use canary_fixtures::LoadedFixture;
use canary_runner::scheduler::{
    build_plan, build_plan_with, CompatibilityPlan, EnabledSurfaces, NetworkMode, SkipCause,
    SkippedFixture,
};
use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};

const XDR_BODY: &str =
    "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n";
const RPC_BODY: &str =
    "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n";

fn loaded(id: &str, protocol: u32, surface: &str, body: &str) -> LoadedFixture {
    canary_fixtures::parse_fixture_str(
        &format!(
            "id = \"{id}\"\nprotocol = {protocol}\nsurface = \"{surface}\"\ncategory = \"c\"\ndescription = \"d\"\n{body}"
        ),
        std::path::Path::new("scheduler_plan.toml"),
    )
    .expect("the fixture metadata and body parse")
}

fn soroban_body() -> String {
    format!(
        "source_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"hello\"\nsequence_number = 1\n\n[expect]\nkind = \"simulation-success\"\n",
        StrkeyPublicKey([0u8; 32]),
        StrkeyContract([0u8; 32]),
    )
}

fn soroban_fixture(id: &str) -> LoadedFixture {
    loaded(id, 28, "soroban", &soroban_body())
}

fn xdr_fixture(id: &str) -> LoadedFixture {
    loaded(id, 28, "xdr", XDR_BODY)
}

fn rpc_fixture(id: &str) -> LoadedFixture {
    loaded(id, 28, "rpc", RPC_BODY)
}

fn project() -> ProjectContext {
    project_with(&[])
}

fn project_with(capabilities: &[Capability]) -> ProjectContext {
    ProjectContext {
        root: ".".into(),
        name: "test".into(),
        project_type: ProjectType::Unknown,
        capabilities: capabilities.to_vec(),
    }
}

fn all_enabled() -> EnabledSurfaces {
    EnabledSurfaces {
        xdr: true,
        rpc: true,
        soroban: true,
    }
}

fn offline() -> NetworkMode {
    NetworkMode::Offline
}

fn causes(plan: &CompatibilityPlan) -> Vec<SkipCause> {
    plan.skipped.iter().map(|skipped| skipped.cause).collect()
}

fn skipped_id<'a>(plan: &'a CompatibilityPlan, id: &str) -> &'a SkippedFixture {
    plan.skipped
        .iter()
        .find(|skipped| skipped.fixture_id == id)
        .unwrap_or_else(|| panic!("{id} should have been skipped, got {:?}", plan.skipped))
}

// -- Live vs Offline transitions ------------------------------------------

#[test]
fn live_mode_schedules_all_three_surfaces() {
    let fixtures = vec![
        xdr_fixture("p28-xdr-live"),
        rpc_fixture("p28-rpc-live"),
        soroban_fixture("p28-soroban-live"),
    ];

    let plan = build_plan_with(
        &fixtures,
        ProtocolVersion(28),
        all_enabled(),
        &project(),
        NetworkMode::Live,
    )
    .expect("planning an all-enabled live run succeeds");

    assert_eq!(plan.xdr.len(), 1);
    assert_eq!(plan.rpc.len(), 1);
    assert_eq!(plan.soroban.len(), 1);
    assert_eq!(plan.applicable_count(), 3);
    assert!(
        plan.skipped.is_empty(),
        "nothing is skipped in live mode: {:?}",
        plan.skipped
    );
    // Per-surface plans keep the loaded fixture order.
    assert_eq!(plan.xdr[0].metadata.id, "p28-xdr-live");
    assert_eq!(plan.rpc[0].metadata.id, "p28-rpc-live");
    assert_eq!(plan.soroban[0].metadata.id, "p28-soroban-live");
}

#[test]
fn offline_mode_keeps_xdr_while_skipping_rpc_and_soroban() {
    let fixtures = vec![
        xdr_fixture("p28-xdr-offline"),
        rpc_fixture("p28-rpc-offline"),
        soroban_fixture("p28-soroban-offline"),
    ];

    let plan = build_plan_with(
        &fixtures,
        ProtocolVersion(28),
        all_enabled(),
        &project(),
        offline(),
    )
    .expect("planning an offline run succeeds");

    assert_eq!(plan.xdr.len(), 1, "offline XDR checks still run");
    assert_eq!(plan.xdr[0].metadata.id, "p28-xdr-offline");
    assert!(plan.rpc.is_empty(), "RPC needs a live endpoint");
    assert!(plan.soroban.is_empty(), "Soroban needs a live endpoint");
    assert_eq!(plan.applicable_count(), 1);

    assert_eq!(plan.skipped.len(), 2);
    assert_eq!(
        causes(&plan),
        [SkipCause::Offline, SkipCause::Offline],
        "skips keep the loaded fixture order"
    );

    let rpc = skipped_id(&plan, "p28-rpc-offline");
    assert_eq!(rpc.surface, Surface::Rpc);
    assert_eq!(rpc.cause.code(), SkipCode::Offline);
    assert!(!rpc.reason.is_empty(), "a skip always explains itself");
    assert!(
        rpc.reason.contains("--offline"),
        "the diagnostic names the flag: {}",
        rpc.reason
    );

    let soroban = skipped_id(&plan, "p28-soroban-offline");
    assert_eq!(soroban.surface, Surface::Soroban);
    assert_eq!(soroban.cause.code(), SkipCode::Offline);
    assert!(
        soroban.reason.contains("--offline"),
        "the diagnostic names the flag: {}",
        soroban.reason
    );
}

#[test]
fn switching_between_live_and_offline_does_not_leak_plan_state() {
    let fixtures = vec![
        xdr_fixture("p28-xdr-transition"),
        rpc_fixture("p28-rpc-transition"),
        soroban_fixture("p28-soroban-transition"),
    ];

    let live = build_plan_with(
        &fixtures,
        ProtocolVersion(28),
        all_enabled(),
        &project(),
        NetworkMode::Live,
    )
    .expect("live planning succeeds");
    assert_eq!(live.applicable_count(), 3);
    assert!(live.skipped.is_empty());

    let offline_plan = build_plan_with(
        &fixtures,
        ProtocolVersion(28),
        all_enabled(),
        &project(),
        offline(),
    )
    .expect("offline planning succeeds");
    assert_eq!(offline_plan.applicable_count(), 1);
    assert_eq!(offline_plan.skipped.len(), 2);

    // And back again: every plan is built from the loaded fixtures alone,
    // so an earlier offline pass cannot suppress a later live one.
    let live_again = build_plan_with(
        &fixtures,
        ProtocolVersion(28),
        all_enabled(),
        &project(),
        NetworkMode::Live,
    )
    .expect("live planning succeeds again");
    assert_eq!(live_again.applicable_count(), 3);
    assert!(live_again.skipped.is_empty());
    assert_eq!(live_again.xdr.len(), 1);
    assert_eq!(live_again.rpc.len(), 1);
    assert_eq!(live_again.soroban.len(), 1);
}

#[test]
fn build_plan_defaults_to_live_mode() {
    // The default entry point must not skip network fixtures: offline is
    // opt-in.
    let fixtures = vec![
        rpc_fixture("p28-rpc-default"),
        soroban_fixture("p28-soroban-default"),
    ];
    let plan = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project())
        .expect("default planning succeeds");

    assert_eq!(plan.applicable_count(), 2);
    assert!(plan.skipped.is_empty());
}

// -- Surface enablement ----------------------------------------------------

#[test]
fn each_surface_can_be_disabled_independently() {
    let fixtures = vec![
        xdr_fixture("p28-xdr-enabled"),
        rpc_fixture("p28-rpc-enabled"),
        soroban_fixture("p28-soroban-enabled"),
    ];

    let cases = [
        (
            EnabledSurfaces {
                xdr: false,
                rpc: true,
                soroban: true,
            },
            Surface::Xdr,
            "p28-xdr-enabled",
        ),
        (
            EnabledSurfaces {
                xdr: true,
                rpc: false,
                soroban: true,
            },
            Surface::Rpc,
            "p28-rpc-enabled",
        ),
        (
            EnabledSurfaces {
                xdr: true,
                rpc: true,
                soroban: false,
            },
            Surface::Soroban,
            "p28-soroban-enabled",
        ),
    ];

    for (enabled, surface, fixture_id) in cases {
        let plan = build_plan_with(
            &fixtures,
            ProtocolVersion(28),
            enabled,
            &project(),
            NetworkMode::Live,
        )
        .expect("planning with one surface disabled succeeds");

        assert_eq!(
            plan.applicable_count(),
            2,
            "{surface:?} disabled should leave two fixtures"
        );
        assert_eq!(plan.skipped.len(), 1, "{surface:?} disabled");

        let skipped = skipped_id(&plan, fixture_id);
        assert_eq!(skipped.surface, surface);
        assert_eq!(skipped.cause, SkipCause::SurfaceDisabled);
        assert_eq!(skipped.cause.code(), SkipCode::SurfaceDisabled);

        // The two other surfaces are still planned.
        assert_eq!(plan.xdr.len(), if surface == Surface::Xdr { 0 } else { 1 });
        assert_eq!(plan.rpc.len(), if surface == Surface::Rpc { 0 } else { 1 });
        assert_eq!(
            plan.soroban.len(),
            if surface == Surface::Soroban { 0 } else { 1 }
        );
    }
}

#[test]
fn disabling_every_surface_yields_an_empty_plan_instead_of_an_error() {
    let fixtures = vec![
        xdr_fixture("p28-xdr-none"),
        rpc_fixture("p28-rpc-none"),
        soroban_fixture("p28-soroban-none"),
    ];
    let none = EnabledSurfaces {
        xdr: false,
        rpc: false,
        soroban: false,
    };

    let plan = build_plan_with(
        &fixtures,
        ProtocolVersion(28),
        none,
        &project(),
        NetworkMode::Live,
    )
    .expect("an all-disabled run is a plan with nothing to run, not an error");

    assert_eq!(plan.applicable_count(), 0);
    assert_eq!(plan.skipped.len(), 3);
    assert_eq!(
        causes(&plan),
        [
            SkipCause::SurfaceDisabled,
            SkipCause::SurfaceDisabled,
            SkipCause::SurfaceDisabled
        ]
    );
}

#[test]
fn a_disabled_surface_keeps_its_specific_cause_under_offline_mode() {
    // Surface enablement is checked before the offline gate, so a disabled
    // RPC surface reports `SurfaceDisabled`, not `Offline`.
    let fixtures = vec![rpc_fixture("p28-rpc-disabled-offline")];
    let rpc_disabled = EnabledSurfaces {
        xdr: true,
        rpc: false,
        soroban: true,
    };

    let plan = build_plan_with(
        &fixtures,
        ProtocolVersion(28),
        rpc_disabled,
        &project(),
        offline(),
    )
    .expect("planning succeeds");

    assert_eq!(plan.applicable_count(), 0);
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].cause, SkipCause::SurfaceDisabled);
}

#[test]
fn offline_mode_only_affects_surfaces_that_are_enabled() {
    // XDR disabled + offline: the XDR fixture is skipped for being disabled
    // and the network fixtures for being offline, in fixture order.
    let fixtures = vec![
        xdr_fixture("p28-xdr-mixed"),
        rpc_fixture("p28-rpc-mixed"),
        soroban_fixture("p28-soroban-mixed"),
    ];
    let xdr_disabled = EnabledSurfaces {
        xdr: false,
        rpc: true,
        soroban: true,
    };

    let plan = build_plan_with(
        &fixtures,
        ProtocolVersion(28),
        xdr_disabled,
        &project(),
        offline(),
    )
    .expect("planning succeeds");

    assert_eq!(plan.applicable_count(), 0);
    assert_eq!(
        causes(&plan),
        [
            SkipCause::SurfaceDisabled,
            SkipCause::Offline,
            SkipCause::Offline
        ]
    );
}

// -- Protocol and capability filtering ------------------------------------

#[test]
fn a_protocol_mismatch_wins_over_the_offline_skip() {
    let fixtures = vec![loaded("p27-rpc-offline", 27, "rpc", RPC_BODY)];

    let plan = build_plan_with(
        &fixtures,
        ProtocolVersion(28),
        all_enabled(),
        &project(),
        offline(),
    )
    .expect("planning succeeds");

    assert_eq!(plan.applicable_count(), 0);
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].cause, SkipCause::ProtocolMismatch);
    assert_eq!(plan.skipped[0].cause.code(), SkipCode::UnsupportedProtocol);
}

#[test]
fn capability_filtering_retains_only_fixtures_the_project_supports() {
    let toml = format!(
        "id = \"p28-soroban-capable\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"c\"\ndescription = \"d\"\nrequired_capabilities = [\"soroban-contract\"]\n{}",
        soroban_body()
    );
    let capable =
        canary_fixtures::parse_fixture_str(&toml, std::path::Path::new("capability.toml"))
            .expect("the capability fixture parses");

    let fixtures = vec![capable.clone(), xdr_fixture("p28-xdr-capable")];

    // A project without the capability keeps only the unconditional fixture.
    let without = build_plan(&fixtures, ProtocolVersion(28), all_enabled(), &project())
        .expect("planning succeeds");
    assert_eq!(without.applicable_count(), 1);
    assert_eq!(without.xdr.len(), 1);
    assert!(without.soroban.is_empty());
    assert_eq!(without.skipped.len(), 1);
    assert_eq!(without.skipped[0].fixture_id, "p28-soroban-capable");
    assert_eq!(without.skipped[0].cause, SkipCause::MissingCapability);
    assert_eq!(without.skipped[0].cause.code(), SkipCode::MissingCapability);

    // Declaring the capability schedules it again.
    let with = build_plan(
        &fixtures,
        ProtocolVersion(28),
        all_enabled(),
        &project_with(&[Capability::SorobanContract]),
    )
    .expect("planning succeeds");
    assert_eq!(with.applicable_count(), 2);
    assert_eq!(with.soroban.len(), 1);
    assert_eq!(with.soroban[0].metadata.id, "p28-soroban-capable");
    assert!(with.skipped.is_empty());
}

#[test]
fn a_missing_capability_wins_over_the_offline_skip() {
    // The capability check runs before the offline gate, so a fixture the
    // project cannot run anyway keeps its more specific reason.
    let toml = format!(
        "id = \"p28-soroban-needy\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"c\"\ndescription = \"d\"\nrequired_capabilities = [\"wasm-artifact\"]\n{}",
        soroban_body()
    );
    let fixture =
        canary_fixtures::parse_fixture_str(&toml, std::path::Path::new("capability-offline.toml"))
            .expect("the capability fixture parses");

    let plan = build_plan_with(
        &[fixture],
        ProtocolVersion(28),
        all_enabled(),
        &project_with(&[Capability::SorobanContract]),
        offline(),
    )
    .expect("planning succeeds");

    assert_eq!(plan.applicable_count(), 0);
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].cause, SkipCause::MissingCapability);
}

#[test]
fn capability_filtering_never_produces_an_empty_plan_without_a_skip_reason() {
    // Every omitted fixture must be accounted for: applicable + skipped is
    // always the number of loaded fixtures.
    let fixtures = vec![
        xdr_fixture("p28-xdr-accounted"),
        rpc_fixture("p28-rpc-accounted"),
        soroban_fixture("p28-soroban-accounted"),
        loaded("p27-xdr-accounted", 27, "xdr", XDR_BODY),
    ];

    for (enabled, mode) in [
        (all_enabled(), NetworkMode::Live),
        (all_enabled(), offline()),
        (
            EnabledSurfaces {
                xdr: false,
                rpc: true,
                soroban: false,
            },
            offline(),
        ),
    ] {
        let plan = build_plan_with(&fixtures, ProtocolVersion(28), enabled, &project(), mode)
            .expect("planning succeeds");
        assert_eq!(
            plan.applicable_count() + plan.skipped.len(),
            fixtures.len(),
            "every loaded fixture is either planned or skipped ({enabled:?}, {mode:?})"
        );
        for skipped in &plan.skipped {
            assert!(!skipped.reason.is_empty());
            assert!(!skipped.fixture_id.is_empty());
        }
    }
}
