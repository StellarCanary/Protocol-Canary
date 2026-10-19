//! Turning raw detection signals into declared [`Capability`] values.

use canary_core::Capability;

use crate::manifest::ProjectManifest;

/// Dependency names that indicate a Soroban contract project.
pub const SOROBAN_DEPENDENCIES: &[&str] = &["soroban-sdk"];

/// The verified Go SDK module path (CF-07 section 4).
pub const GO_STELLAR_SDK_DEPENDENCY: &str = "github.com/stellar/go-stellar-sdk";

/// Dependency names that indicate direct use of official Stellar SDK/XDR
/// crates without necessarily being a Soroban contract.
pub const STELLAR_SDK_DEPENDENCIES: &[&str] = &[
    "stellar-sdk",
    "stellar-xdr",
    "stellar-base",
    "stellar-strkey",
    "@stellar/stellar-sdk",
    "@stellar/stellar-base",
    GO_STELLAR_SDK_DEPENDENCY,
];

/// Dependency names that indicate the project talks to Stellar RPC.
pub const RPC_CLIENT_DEPENDENCIES: &[&str] = &[
    "stellar-rpc-client",
    "soroban-rpc",
    "@stellar/stellar-sdk", // JS SDK includes RPC client
    "soroban-client",
];

/// The raw, filesystem-level signals gathered for a project root.
#[derive(Debug, Default, Clone)]
pub struct DetectionSignals {
    pub manifests: Vec<ProjectManifest>,
    pub has_stellar_toml: bool,
    pub has_wasm_artifact: bool,
}

/// Derives the set of declared [`Capability`] values from raw signals.
pub fn detect_capabilities(signals: &DetectionSignals) -> Vec<Capability> {
    let mut capabilities = Vec::new();

    for manifest in &signals.manifests {
        if manifest.has_any_dependency(SOROBAN_DEPENDENCIES)
            && !capabilities.contains(&Capability::SorobanContract)
        {
            capabilities.push(Capability::SorobanContract);
        }
        if manifest.has_any_dependency(STELLAR_SDK_DEPENDENCIES)
            && !capabilities.contains(&Capability::StellarSdkDependency)
        {
            capabilities.push(Capability::StellarSdkDependency);
        }
        if manifest.has_any_dependency(RPC_CLIENT_DEPENDENCIES)
            && !capabilities.contains(&Capability::RpcClient)
        {
            capabilities.push(Capability::RpcClient);
        }
    }

    if signals.has_wasm_artifact {
        capabilities.push(Capability::WasmArtifact);
    }

    capabilities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soroban_dependency_yields_soroban_contract_capability() {
        let signals = DetectionSignals {
            manifests: vec![ProjectManifest {
                dependency_names: vec!["soroban-sdk".to_string()],
            }],
            ..Default::default()
        };
        assert!(detect_capabilities(&signals).contains(&Capability::SorobanContract));
    }

    #[test]
    fn js_stellar_sdk_dependency_yields_capabilities() {
        let signals = DetectionSignals {
            manifests: vec![ProjectManifest {
                dependency_names: vec!["@stellar/stellar-sdk".to_string()],
            }],
            ..Default::default()
        };
        let caps = detect_capabilities(&signals);
        assert!(caps.contains(&Capability::StellarSdkDependency));
        assert!(caps.contains(&Capability::RpcClient));
    }

    #[test]
    fn no_signals_yield_no_capabilities() {
        assert!(detect_capabilities(&DetectionSignals::default()).is_empty());
    }

    #[test]
    fn stellar_rpc_client_dependency_yields_rpc_client_capability() {
        let signals = DetectionSignals {
            manifests: vec![ProjectManifest {
                dependency_names: vec!["stellar-rpc-client".to_string()],
            }],
            ..Default::default()
        };
        assert!(detect_capabilities(&signals).contains(&Capability::RpcClient));
    }

    #[test]
    fn soroban_rpc_dependency_yields_rpc_client_capability() {
        let signals = DetectionSignals {
            manifests: vec![ProjectManifest {
                dependency_names: vec!["soroban-rpc".to_string()],
            }],
            ..Default::default()
        };
        assert!(detect_capabilities(&signals).contains(&Capability::RpcClient));
    }

    #[test]
    fn wasm_artifact_signal_yields_wasm_artifact_capability() {
        let signals = DetectionSignals {
            has_wasm_artifact: true,
            ..Default::default()
        };
        assert_eq!(
            detect_capabilities(&signals),
            vec![Capability::WasmArtifact]
        );
    }

    #[test]
    fn go_stellar_sdk_dependency_yields_stellar_sdk_capability() {
        let signals = DetectionSignals {
            manifests: vec![ProjectManifest {
                dependency_names: vec![GO_STELLAR_SDK_DEPENDENCY.to_string()],
            }],
            ..Default::default()
        };
        let caps = detect_capabilities(&signals);
        assert_eq!(caps, vec![Capability::StellarSdkDependency]);
    }
}
