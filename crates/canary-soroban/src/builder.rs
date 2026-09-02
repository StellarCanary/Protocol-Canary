//! Building an unsigned Soroban `InvokeHostFunction` transaction envelope.
//!
//! This never signs anything and never needs a secret key: Stellar RPC's
//! `simulateTransaction` only needs an unsigned envelope, and this project
//! never submits a transaction (see the project's no-private-key and
//! no-transaction-submission rules).

use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
use stellar_xdr::{
    ContractId, Hash, HostFunction, InvokeContractArgs, InvokeHostFunctionOp, Limits, Memo,
    MuxedAccount, Operation, OperationBody, Preconditions, ScAddress, ScString, ScSymbol, ScVal,
    SequenceNumber, Transaction, TransactionEnvelope, TransactionExt, TransactionV1Envelope,
    Uint256, VecM, WriteXdr,
};

#[derive(Debug, thiserror::Error)]
pub enum BuilderError {
    #[error("invalid source account strkey {account:?}: {reason}")]
    InvalidSourceAccount { account: String, reason: String },

    #[error("invalid contract strkey {contract:?}: {reason}")]
    InvalidContractId { contract: String, reason: String },

    #[error("invalid function name {name:?}: {reason}")]
    InvalidFunctionName { name: String, reason: String },

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("failed to encode transaction envelope: {0}")]
    Encode(String),
}

/// A Soroban value a fixture can pass as a contract function argument.
///
/// This intentionally supports a small set of scalar types — enough for
/// the fixtures this project needs — rather than the full `ScVal` union.
#[derive(Debug, Clone, PartialEq)]
pub enum ScValInput {
    Bool(bool),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    Symbol(String),
    String(String),
}

/// Everything needed to build an unsigned `InvokeHostFunction` transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct InvocationSpec {
    pub source_account: String,
    pub contract_id: String,
    pub function_name: String,
    pub args: Vec<ScValInput>,
    pub sequence_number: i64,
}

fn to_scval(input: &ScValInput) -> Result<ScVal, BuilderError> {
    Ok(match input {
        ScValInput::Bool(b) => ScVal::Bool(*b),
        ScValInput::U32(v) => ScVal::U32(*v),
        ScValInput::I32(v) => ScVal::I32(*v),
        ScValInput::U64(v) => ScVal::U64(*v),
        ScValInput::I64(v) => ScVal::I64(*v),
        ScValInput::Symbol(s) => {
            ScVal::Symbol(ScSymbol(s.as_str().try_into().map_err(
                |e: stellar_xdr::Error| BuilderError::InvalidArgument(e.to_string()),
            )?))
        }
        ScValInput::String(s) => {
            ScVal::String(ScString(s.as_str().try_into().map_err(
                |e: stellar_xdr::Error| BuilderError::InvalidArgument(e.to_string()),
            )?))
        }
    })
}

/// Builds an unsigned `TransactionEnvelope` invoking a Soroban contract
/// function, encoded as base64 XDR ready to pass to `simulateTransaction`.
pub fn build_invoke_transaction_envelope(spec: &InvocationSpec) -> Result<String, BuilderError> {
    let source_bytes = StrkeyPublicKey::from_string(&spec.source_account)
        .map_err(|e| BuilderError::InvalidSourceAccount {
            account: spec.source_account.clone(),
            reason: e.to_string(),
        })?
        .0;
    let contract_bytes = StrkeyContract::from_string(&spec.contract_id)
        .map_err(|e| BuilderError::InvalidContractId {
            contract: spec.contract_id.clone(),
            reason: e.to_string(),
        })?
        .0;
    let function_name: ScSymbol = ScSymbol(spec.function_name.as_str().try_into().map_err(
        |e: stellar_xdr::Error| BuilderError::InvalidFunctionName {
            name: spec.function_name.clone(),
            reason: e.to_string(),
        },
    )?);

    let mut args = Vec::with_capacity(spec.args.len());
    for input in &spec.args {
        args.push(to_scval(input)?);
    }

    let invoke_args = InvokeContractArgs {
        contract_address: ScAddress::Contract(ContractId(Hash(contract_bytes))),
        function_name,
        args: args
            .try_into()
            .map_err(|e: stellar_xdr::Error| BuilderError::InvalidArgument(e.to_string()))?,
    };

    let operation = Operation {
        source_account: None,
        body: OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
            host_function: HostFunction::InvokeContract(invoke_args),
            auth: VecM::default(),
        }),
    };

    let transaction = Transaction {
        source_account: MuxedAccount::Ed25519(Uint256(source_bytes)),
        fee: 100,
        seq_num: SequenceNumber(spec.sequence_number),
        cond: Preconditions::None,
        memo: Memo::None,
        operations: vec![operation]
            .try_into()
            .map_err(|e: stellar_xdr::Error| BuilderError::Encode(e.to_string()))?,
        ext: TransactionExt::V0,
    };

    let envelope = TransactionEnvelope::Tx(TransactionV1Envelope {
        tx: transaction,
        signatures: VecM::default(),
    });

    envelope
        .to_xdr_base64(Limits::none())
        .map_err(|e| BuilderError::Encode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A well-formed but arbitrary account/contract pair, encoded from all-
    /// zero payload bytes purely to exercise strkey decoding; these do not
    /// need to exist on any real network for envelope-construction tests.
    fn spec() -> InvocationSpec {
        let source_account = StrkeyPublicKey([0u8; 32]).to_string();
        let contract_id = StrkeyContract([0u8; 32]).to_string();
        InvocationSpec {
            source_account,
            contract_id,
            function_name: "hello".to_string(),
            args: vec![ScValInput::Symbol("world".to_string())],
            sequence_number: 1,
        }
    }

    #[test]
    fn builds_a_valid_base64_envelope() {
        let base64 = build_invoke_transaction_envelope(&spec()).expect("builds");
        assert!(!base64.is_empty());

        // The envelope must itself decode back with stellar-xdr, proving
        // it is well-formed XDR, not just a non-empty string.
        use stellar_xdr::{Limits, ReadXdr};
        TransactionEnvelope::from_xdr_base64(&base64, Limits::none())
            .expect("the built envelope must be valid XDR");
    }

    #[test]
    fn rejects_an_invalid_source_account() {
        let mut spec = spec();
        spec.source_account = "not-a-strkey".to_string();
        let err = build_invoke_transaction_envelope(&spec).unwrap_err();
        assert!(matches!(err, BuilderError::InvalidSourceAccount { .. }));
    }

    #[test]
    fn rejects_an_invalid_contract_id() {
        let mut spec = spec();
        spec.contract_id = "not-a-strkey".to_string();
        let err = build_invoke_transaction_envelope(&spec).unwrap_err();
        assert!(matches!(err, BuilderError::InvalidContractId { .. }));
    }

    #[test]
    fn supports_scalar_argument_types() {
        let mut spec = spec();
        spec.args = vec![
            ScValInput::Bool(true),
            ScValInput::U32(1),
            ScValInput::I32(-1),
            ScValInput::U64(2),
            ScValInput::I64(-2),
            ScValInput::String("hi".to_string()),
        ];
        assert!(build_invoke_transaction_envelope(&spec).is_ok());
    }
}
