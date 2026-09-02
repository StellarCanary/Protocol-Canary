//! Turning raw detection signals into declared [`Capability`] values.

use canary_core::Capability;

use crate::manifest::CargoManifest;

/// Dependency names that indicate a Soroban contract project.
const SOROBAN_DEPENDENCIES: &[&str] = &["soroban-sdk"];

/// Dependency names that indicate direct use of official Stellar SDK/XDR
/// crates without necessarily being a Soroban contract.
const STELLAR_SDK_DEPENDENCIES: &[&str] = &[
    "stellar-sdk",
    "stellar-xdr",
    "stellar-base",
    "stellar-strkey",
];

/// Dependency names that indicate the project talks to Stellar RPC.
const RPC_CLIENT_DEPENDENCIES: &[&str] = &["stellar-rpc-client", "soroban-rpc"];

/// The raw, filesystem-level signals gathered for a project root.
#[derive(Debug, Default, Clone)]
pub struct DetectionSignals {
    pub manifest: Option<CargoManifest>,
    pub has_stellar_toml: bool,
    pub has_wasm_artifact: bool,
}

/// Derives the set of declared [`Capability`] values from raw signals.
pub fn detect_capabilities(signals: &DetectionSignals) -> Vec<Capability> {
    let mut capabilities = Vec::new();

    if let Some(manifest) = &signals.manifest {
        if manifest.has_any_dependency(SOROBAN_DEPENDENCIES) {
            capabilities.push(Capability::SorobanContract);
        }
        if manifest.has_any_dependency(STELLAR_SDK_DEPENDENCIES) {
            capabilities.push(Capability::StellarSdkDependency);
        }
        if manifest.has_any_dependency(RPC_CLIENT_DEPENDENCIES) {
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
            manifest: Some(CargoManifest {
                dependency_names: vec!["soroban-sdk".to_string()],
            }),
            ..Default::default()
        };
        assert!(detect_capabilities(&signals).contains(&Capability::SorobanContract));
    }

    #[test]
    fn no_signals_yield_no_capabilities() {
        assert!(detect_capabilities(&DetectionSignals::default()).is_empty());
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
}
