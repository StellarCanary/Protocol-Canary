//! Building an unsigned Soroban `InvokeHostFunction` transaction envelope.
//!
//! This never signs anything and never needs a secret key: Stellar RPC's
//! `simulateTransaction` only needs an unsigned envelope, and this project
//! never submits a transaction (see the project's no-private-key and
//! no-transaction-submission rules).

use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
use stellar_xdr::{
    ContractId, Duration, Hash, HostFunction, Int128Parts, Int256Parts, InvokeContractArgs,
    InvokeHostFunctionOp, Limits, Memo, MuxedAccount, Operation, OperationBody, Preconditions,
    ScAddress, ScString, ScSymbol, ScVal, SequenceNumber, TimePoint, Transaction,
    TransactionEnvelope, TransactionExt, TransactionV1Envelope, UInt128Parts, UInt256Parts,
    Uint256, VecM, WriteXdr,
};

/// Errors returned while building an unsigned Soroban invocation envelope.
///
/// These errors indicate invalid account, contract, function, or argument
/// input, or a failure to encode the resulting transaction as XDR. The builder
/// returns these errors rather than panicking.
#[derive(Debug, thiserror::Error)]
pub enum BuilderError {
    /// The source account is not a valid Stellar account strkey.
    #[error("invalid source account strkey {account:?}: {reason}")]
    InvalidSourceAccount { account: String, reason: String },

    /// The contract identifier is not a valid Stellar contract strkey.
    #[error("invalid contract strkey {contract:?}: {reason}")]
    InvalidContractId { contract: String, reason: String },

    /// The function name cannot be represented as a Soroban symbol.
    #[error("invalid function name {name:?}: {reason}")]
    InvalidFunctionName { name: String, reason: String },

    /// An argument cannot be represented in the supported Soroban value format.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// An integer argument string does not fit in the target integer width
    /// (128 or 256 bits).
    #[error("integer overflow: value {value:?} does not fit in a {bits}-bit integer")]
    IntegerOverflowError { value: String, bits: u16 },

    /// The transaction envelope could not be encoded as base64 XDR.
    #[error("failed to encode transaction envelope: {0}")]
    Encode(String),
}

/// A Soroban value a fixture can pass as a contract function argument.
///
/// This intentionally supports a small set of scalar types — enough for
/// the fixtures this project needs — rather than the full `ScVal` union.
#[derive(Debug, Clone, PartialEq)]
pub enum ScValInput {
    /// Maps to the XDR [`ScVal::Bool`] variant.
    Bool(bool),
    /// Maps to the XDR [`ScVal::U32`] variant.
    U32(u32),
    /// Maps to the XDR [`ScVal::I32`] variant.
    I32(i32),
    /// Maps to the XDR [`ScVal::U64`] variant.
    U64(u64),
    /// Maps to the XDR [`ScVal::I64`] variant.
    I64(i64),
    /// Maps to the XDR [`ScVal::I128`] variant.
    ///
    /// The value is a decimal or `0x`-prefixed hexadecimal string; TOML
    /// integers cannot express 128-bit ranges, so the raw string is parsed
    /// when the envelope is built.
    I128(String),
    /// Maps to the XDR [`ScVal::U128`] variant; decimal or `0x`-prefixed hex string.
    U128(String),
    /// Maps to the XDR [`ScVal::I256`] variant; decimal or `0x`-prefixed hex string.
    I256(String),
    /// Maps to the XDR [`ScVal::U256`] variant; decimal or `0x`-prefixed hex string.
    U256(String),
    /// Maps to the XDR [`ScVal::Void`] variant; carries no payload.
    Void,
    /// Maps to the XDR [`ScVal::Duration`] variant (an unsigned 64-bit count
    /// of seconds).
    Duration(u64),
    /// Maps to the XDR [`ScVal::Timepoint`] variant (an unsigned 64-bit Unix
    /// timestamp).
    Timepoint(u64),
    /// Maps to the XDR [`ScVal::Symbol`] variant; the symbol is limited to 32 bytes by XDR.
    Symbol(String),
    /// Maps to the XDR [`ScVal::String`] variant.
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

/// Splits a numeric argument literal into (negative, radix, digits).
///
/// Accepts an optional leading sign (`-` or `+`) and an optional `0x`/`0X`
/// prefix (after the sign) selecting base 16; anything else is base 10.
/// The returned `digits` carry no sign and no radix prefix, so callers parse
/// them as a magnitude and apply `negative` themselves.
fn split_integer_literal(value: &str) -> (bool, u32, &str) {
    let (negative, unsigned) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let (radix, digits) = match unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        Some(hex) => (16, hex),
        None => (10, unsigned),
    };
    (negative, radix, digits)
}

/// Maps a [`std::num::ParseIntError`] onto the builder's error type,
/// distinguishing an out-of-range value (overflow) from a malformed one.
fn integer_parse_error(error: std::num::ParseIntError, value: &str, bits: u16) -> BuilderError {
    match error.kind() {
        std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow => {
            BuilderError::IntegerOverflowError {
                value: value.to_string(),
                bits,
            }
        }
        _ => BuilderError::InvalidArgument(format!(
            "{value:?} is not a valid {bits}-bit integer: {error}"
        )),
    }
}

/// Parses a 128-bit unsigned decimal or `0x`-prefixed hex string.
fn parse_u128_literal(value: &str) -> Result<u128, BuilderError> {
    let (negative, radix, digits) = split_integer_literal(value);
    if negative {
        return Err(BuilderError::InvalidArgument(format!(
            "{value:?} must not be negative"
        )));
    }
    u128::from_str_radix(digits, radix).map_err(|e| integer_parse_error(e, value, 128))
}

/// Parses a 128-bit signed decimal or `0x`-prefixed hex string, rejecting
/// magnitudes outside the `i128` range with [`BuilderError::IntegerOverflowError`].
fn parse_i128_literal(value: &str) -> Result<i128, BuilderError> {
    let (negative, radix, digits) = split_integer_literal(value);
    let magnitude =
        u128::from_str_radix(digits, radix).map_err(|e| integer_parse_error(e, value, 128))?;
    if negative {
        // The most negative i128 has magnitude 2^127, which does not fit in
        // i128 itself, so it is checked (and produced) separately.
        if magnitude > 1u128 << 127 {
            return Err(BuilderError::IntegerOverflowError {
                value: value.to_string(),
                bits: 128,
            });
        }
        if magnitude == 1u128 << 127 {
            return Ok(i128::MIN);
        }
        Ok(-(magnitude as i128))
    } else {
        if magnitude > i128::MAX as u128 {
            return Err(BuilderError::IntegerOverflowError {
                value: value.to_string(),
                bits: 128,
            });
        }
        Ok(magnitude as i128)
    }
}

/// Parses a magnitude of up to 256 bits into four little-endian 64-bit limbs
/// (`[lo_lo, lo_hi, hi_lo, hi_hi]`), rejecting values with more than 256
/// bits and characters that are not valid digits for `radix`.
fn parse_u256_magnitude(digits: &str, radix: u32, value: &str) -> Result<[u64; 4], BuilderError> {
    if digits.is_empty() {
        return Err(BuilderError::InvalidArgument(format!(
            "{value:?} is not a valid 256-bit integer"
        )));
    }
    let mut limbs = [0u64; 4];
    for character in digits.chars() {
        let digit = character.to_digit(radix).ok_or_else(|| {
            BuilderError::InvalidArgument(format!(
                "{value:?} is not a valid 256-bit integer: invalid digit {character:?}"
            ))
        })?;
        let mut carry = u128::from(digit);
        for limb in &mut limbs {
            let product = u128::from(*limb) * u128::from(radix) + carry;
            *limb = product as u64;
            carry = product >> 64;
        }
        if carry != 0 {
            return Err(BuilderError::IntegerOverflowError {
                value: value.to_string(),
                bits: 256,
            });
        }
    }
    Ok(limbs)
}

/// Parses a 256-bit unsigned decimal or `0x`-prefixed hex string.
fn parse_u256_literal(value: &str) -> Result<UInt256Parts, BuilderError> {
    let (negative, radix, digits) = split_integer_literal(value);
    if negative {
        return Err(BuilderError::InvalidArgument(format!(
            "{value:?} must not be negative"
        )));
    }
    let [lo_lo, lo_hi, hi_lo, hi_hi] = parse_u256_magnitude(digits, radix, value)?;
    Ok(UInt256Parts {
        hi_hi,
        hi_lo,
        lo_hi,
        lo_lo,
    })
}

/// Parses a 256-bit signed decimal or `0x`-prefixed hex string. Negative
/// magnitudes are converted to the two's-complement representation used by
/// XDR; magnitudes above 2^255 are rejected as overflow.
fn parse_i256_literal(value: &str) -> Result<Int256Parts, BuilderError> {
    let (negative, radix, digits) = split_integer_literal(value);
    let magnitude = parse_u256_magnitude(digits, radix, value)?;
    if !negative {
        if magnitude[3] & (1 << 63) != 0 {
            return Err(BuilderError::IntegerOverflowError {
                value: value.to_string(),
                bits: 256,
            });
        }
        return Ok(Int256Parts {
            hi_hi: magnitude[3] as i64,
            hi_lo: magnitude[2],
            lo_hi: magnitude[1],
            lo_lo: magnitude[0],
        });
    }
    // A negative i256 can represent magnitudes up to and including 2^255
    // (its most negative value); anything larger overflows.
    let sign_word = 1u64 << 63;
    if magnitude[3] > sign_word
        || (magnitude[3] == sign_word && (magnitude[2] | magnitude[1] | magnitude[0]) != 0)
    {
        return Err(BuilderError::IntegerOverflowError {
            value: value.to_string(),
            bits: 256,
        });
    }
    // Two's complement: invert all limbs and add one.
    let mut complement = [0u64; 4];
    let mut carry = 1u64;
    for (out, limb) in complement.iter_mut().zip(magnitude) {
        let (sum, overflowed) = (!limb).overflowing_add(carry);
        *out = sum;
        carry = u64::from(overflowed);
    }
    Ok(Int256Parts {
        hi_hi: complement[3] as i64,
        hi_lo: complement[2],
        lo_hi: complement[1],
        lo_lo: complement[0],
    })
}

fn to_scval(input: &ScValInput) -> Result<ScVal, BuilderError> {
    Ok(match input {
        ScValInput::Bool(b) => ScVal::Bool(*b),
        ScValInput::U32(v) => ScVal::U32(*v),
        ScValInput::I32(v) => ScVal::I32(*v),
        ScValInput::U64(v) => ScVal::U64(*v),
        ScValInput::I64(v) => ScVal::I64(*v),
        ScValInput::I128(value) => {
            let parsed = parse_i128_literal(value)?;
            ScVal::I128(Int128Parts {
                hi: (parsed >> 64) as i64,
                lo: parsed as u64,
            })
        }
        ScValInput::U128(value) => {
            let parsed = parse_u128_literal(value)?;
            ScVal::U128(UInt128Parts {
                hi: (parsed >> 64) as u64,
                lo: parsed as u64,
            })
        }
        ScValInput::I256(value) => ScVal::I256(parse_i256_literal(value)?),
        ScValInput::U256(value) => ScVal::U256(parse_u256_literal(value)?),
        ScValInput::Void => ScVal::Void,
        ScValInput::Duration(v) => ScVal::Duration(Duration(*v)),
        ScValInput::Timepoint(v) => ScVal::Timepoint(TimePoint(*v)),
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
            source_account: source_account.to_string(),
            contract_id: contract_id.to_string(),
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
    fn rejects_a_function_name_longer_than_the_xdr_symbol_limit() {
        // ScSymbol is limited to 32 bytes of UTF-8, so a 33-character name
        // must fail conversion and surface as `InvalidFunctionName`.
        let mut spec = spec();
        spec.function_name = "a".repeat(33);
        assert_eq!(spec.function_name.len(), 33);
        let err = build_invoke_transaction_envelope(&spec).unwrap_err();
        assert!(matches!(err, BuilderError::InvalidFunctionName { .. }));
    }

    #[test]
    fn supports_scalar_argument_types() {
        use stellar_xdr::ReadXdr;

        let mut spec = spec();
        spec.args = vec![
            ScValInput::Bool(true),
            ScValInput::U32(1),
            ScValInput::I32(-1),
            ScValInput::U64(2),
            ScValInput::I64(-2),
            ScValInput::String("hi".to_string()),
        ];
        let base64 = build_invoke_transaction_envelope(&spec).expect("builds");
        let envelope = TransactionEnvelope::from_xdr_base64(&base64, Limits::none())
            .expect("the built envelope must be valid XDR");
        let TransactionEnvelope::Tx(envelope) = envelope else {
            panic!("expected a transaction envelope");
        };
        let operation = envelope.tx.operations.first().expect("one operation");
        let OperationBody::InvokeHostFunction(invoke) = &operation.body else {
            panic!("expected an invoke host function operation");
        };
        let HostFunction::InvokeContract(invoke_args) = &invoke.host_function else {
            panic!("expected an invoke contract host function");
        };

        assert_eq!(
            &invoke_args.args[..],
            &[
                ScVal::Bool(true),
                ScVal::U32(1),
                ScVal::I32(-1),
                ScVal::U64(2),
                ScVal::I64(-2),
                ScVal::String(ScString("hi".try_into().expect("valid XDR string"))),
            ]
        );
    }

    #[test]
    fn converts_a_symbol_argument_to_the_xdr_symbol_variant() {
        use stellar_xdr::ReadXdr;

        let mut spec = spec();
        spec.args = vec![ScValInput::Symbol("transfer".to_string())];
        let base64 = build_invoke_transaction_envelope(&spec).expect("builds");
        let envelope = TransactionEnvelope::from_xdr_base64(&base64, Limits::none())
            .expect("the built envelope must be valid XDR");
        let TransactionEnvelope::Tx(envelope) = envelope else {
            panic!("expected a transaction envelope");
        };
        let operation = envelope.tx.operations.first().expect("one operation");
        let OperationBody::InvokeHostFunction(invoke) = &operation.body else {
            panic!("expected an invoke host function operation");
        };
        let HostFunction::InvokeContract(invoke_args) = &invoke.host_function else {
            panic!("expected an invoke contract host function");
        };

        assert_eq!(
            &invoke_args.args[..],
            &[ScVal::Symbol(ScSymbol(
                "transfer".try_into().expect("valid XDR symbol")
            ))]
        );
    }
}
