//! Calling `simulateTransaction` for a built envelope.

use canary_rpc::{RpcClient, RpcError, SimulationRequest, SimulationResponse};

/// Simulates `envelope_base64` (an unsigned `TransactionEnvelope`) against
/// `client`. This never signs or submits anything.
pub async fn simulate(
    client: &impl RpcClient,
    envelope_base64: String,
) -> Result<SimulationResponse, RpcError> {
    client
        .simulate_transaction(SimulationRequest {
            transaction: envelope_base64,
        })
        .await
}
