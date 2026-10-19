//! Turning raw detection signals into declared [`Capability`] values.

use canary_core::Capability;

use crate::manifest::ProjectManifest;
use crate::models::{CapabilityEvidence, CapabilityModel};

pub use crate::models::{CapabilityEvidence as ModelEvidence, CapabilityModel as Model};

/// Dependency names that indicate a Soroban contract project.
pub const SOROBAN_DEPENDENCIES: &[&str] = &["soroban-sdk"];

/// Dependency names that indicate direct use of official Stellar SDK/XDR
/// crates without necessarily being a Soroban contract.
pub const STELLAR_SDK_DEPENDENCIES: &[&str] = &[
    "stellar-sdk",
    "stellar-xdr",
    "stellar-base",
    "stellar-strkey",
    "@stellar/stellar-sdk",
    "@stellar/stellar-base",
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

/// Derives the declared [`CapabilityModel`] values with populated evidence records from raw signals.
pub fn detect_capabilities_models(signals: &DetectionSignals) -> Vec<CapabilityModel> {
    let mut models: Vec<CapabilityModel> = Vec::new();

    for manifest in &signals.manifests {
        for dep in &manifest.dependency_names {
            if SOROBAN_DEPENDENCIES.contains(&dep.as_str()) {
                add_evidence_record(
                    &mut models,
                    Capability::SorobanContract,
                    CapabilityEvidence::new(
                        "manifest",
                        dep,
                        "soroban-sdk dependency declared in project manifest",
                    ),
                );
            }
            if STELLAR_SDK_DEPENDENCIES.contains(&dep.as_str()) {
                add_evidence_record(
                    &mut models,
                    Capability::StellarSdkDependency,
                    CapabilityEvidence::new(
                        "manifest",
                        dep,
                        "stellar SDK dependency declared in project manifest",
                    ),
                );
            }
            if RPC_CLIENT_DEPENDENCIES.contains(&dep.as_str()) {
                add_evidence_record(
                    &mut models,
                    Capability::RpcClient,
                    CapabilityEvidence::new(
                        "manifest",
                        dep,
                        "stellar RPC client dependency declared in project manifest",
                    ),
                );
            }
        }
    }

    if signals.has_wasm_artifact {
        add_evidence_record(
            &mut models,
            Capability::WasmArtifact,
            CapabilityEvidence::new(
                "artifact",
                "*.wasm",
                "compiled Soroban WASM artifact found in build output",
            ),
        );
    }

    models
}

fn add_evidence_record(
    models: &mut Vec<CapabilityModel>,
    capability: Capability,
    evidence: CapabilityEvidence,
) {
    if let Some(existing) = models.iter_mut().find(|m| m.capability == capability) {
        if !existing.evidence.contains(&evidence) {
            existing.evidence.push(evidence);
        }
    } else {
        models.push(CapabilityModel::new(capability, vec![evidence]));
    }
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
    fn detect_capabilities_models_populates_evidence_records() {
        let signals = DetectionSignals {
            manifests: vec![ProjectManifest {
                dependency_names: vec!["soroban-sdk".to_string(), "stellar-sdk".to_string()],
            }],
            has_wasm_artifact: true,
            ..Default::default()
        };
        let models = detect_capabilities_models(&signals);
        assert_eq!(models.len(), 3);

        let soroban = models
            .iter()
            .find(|m| m.capability == Capability::SorobanContract)
            .expect("soroban capability model");
        assert_eq!(soroban.evidence.len(), 1);
        assert_eq!(soroban.evidence[0].source_file, "manifest");
        assert_eq!(soroban.evidence[0].matched_text, "soroban-sdk");
        assert!(!soroban.evidence[0].reason.is_empty());

        let wasm = models
            .iter()
            .find(|m| m.capability == Capability::WasmArtifact)
            .expect("wasm capability model");
        assert_eq!(wasm.evidence.len(), 1);
        assert_eq!(wasm.evidence[0].matched_text, "*.wasm");
    }

    #[test]
    fn negative_empty_signals_produce_zero_capabilities_and_zero_evidence() {
        let signals = DetectionSignals::default();
        let models = detect_capabilities_models(&signals);
        assert!(models.is_empty());
        assert!(detect_capabilities(&signals).is_empty());
    }
}
