//! Project detection models and capability evidence structures (CA-04, CF-07, issue #412).

use canary_core::Capability;
use serde::{Deserialize, Serialize};

/// Evidence supporting why a capability was assigned to a project per CA-04 / CF-07.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityEvidence {
    /// Normalized root-relative path to the manifest or artifact file.
    pub source_file: String,
    /// The matched dependency, string, or artifact identifier.
    pub matched_text: String,
    /// The reason or manifest section explaining why this evidence produces the capability.
    pub reason: String,
}

impl CapabilityEvidence {
    /// Creates a new capability evidence record.
    pub fn new(
        source_file: impl Into<String>,
        matched_text: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            source_file: source_file.into(),
            matched_text: matched_text.into(),
            reason: reason.into(),
        }
    }
}

/// A capability model associating a [`Capability`] with its discovered evidence records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityModel {
    pub capability: Capability,
    pub evidence: Vec<CapabilityEvidence>,
}

impl CapabilityModel {
    /// Creates a new capability model with its supporting evidence.
    pub fn new(capability: Capability, evidence: Vec<CapabilityEvidence>) -> Self {
        Self {
            capability,
            evidence,
        }
    }

    /// Returns `true` if this model has at least one evidence record.
    pub fn has_evidence(&self) -> bool {
        !self.evidence.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_evidence_creation_and_fields() {
        let ev = CapabilityEvidence::new("Cargo.toml", "soroban-sdk", "dependencies declared");
        assert_eq!(ev.source_file, "Cargo.toml");
        assert_eq!(ev.matched_text, "soroban-sdk");
        assert_eq!(ev.reason, "dependencies declared");
    }

    #[test]
    fn capability_model_creation_and_evidence() {
        let ev = CapabilityEvidence::new("Cargo.toml", "soroban-sdk", "dependencies declared");
        let model = CapabilityModel::new(Capability::SorobanContract, vec![ev]);
        assert_eq!(model.capability, Capability::SorobanContract);
        assert_eq!(model.evidence.len(), 1);
        assert!(model.has_evidence());
    }
}
