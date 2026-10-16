//! Building an unsigned Soroban `InvokeHostFunction` transaction envelope.
//!
//! This never signs anything and never needs a secret key: Stellar RPC's
//! `simulateTransaction` only needs an unsigned envelope, and this project
//! never submits a transaction (see the project's no-private-key and
//! no-transaction-submission rules).

use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract, Strkey};
use stellar_xdr::{
    AccountId, ContractId, Duration, Hash, HostFunction, Int128Parts, Int256Parts,
    InvokeContractArgs, InvokeHostFunctionOp, Limits, Memo, MuxedAccount, Operation, OperationBody,
    Preconditions, PublicKey, ScAddress, ScBytes, ScMap, ScMapEntry, ScString, ScSymbol, ScVal,
    ScVec, SequenceNumber, TimePoint, Transaction, TransactionEnvelope, TransactionExt,
    TransactionV1Envelope, UInt128Parts, UInt256Parts, Uint256, VecM, WriteXdr,
};

/// Deepest nesting of `Vec`/`Map` containers an argument may have.
///
/// The limit bounds the recursion of the builder, so a hostile or mistaken
/// fixture cannot overflow the stack. A top-level `Vec` is at depth 1.
pub const MAX_ARG_DEPTH: usize = 8;

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

    /// A `Bytes` or `Address` argument could not be converted: malformed
    /// hex or base64, or a string that is not a `G...` account or `C...`
    /// contract address (including a bad strkey checksum).
    #[error("argument conversion error ({kind}): {reason}")]
    ArgumentConversionError { kind: &'static str, reason: String },

    /// `Vec`/`Map` arguments are nested more deeply than [`MAX_ARG_DEPTH`].
    #[error("argument nesting is deeper than the limit of {limit} levels")]
    RecursionDepthExceeded { limit: usize },

    /// A `Map` argument has two entries with the same key. Soroban maps are
    /// keyed collections, so a repeated key is ambiguous.
    #[error("duplicate key in map argument")]
    DuplicateMapKey,

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
    /// Maps to the XDR [`ScVal::Bytes`] variant.
    ///
    /// The value is hexadecimal when it starts with `0x` or `0X`, and
    /// standard padded base64 otherwise. The prefix removes the ambiguity of
    /// a string such as `abcd`, which is valid in both alphabets.
    Bytes(String),
    /// Maps to the XDR [`ScVal::Address`] variant. A `G...` account strkey
    /// becomes an account address and a `C...` strkey a contract address.
    /// Secret seeds and other strkey kinds are rejected.
    Address(String),
    /// Maps to the XDR [`ScVal::Vec`] variant. Nesting is limited to
    /// [`MAX_ARG_DEPTH`] levels.
    Vec(Vec<ScValInput>),
    /// Maps to the XDR [`ScVal::Map`] variant, as `(key, value)` pairs.
    ///
    /// Soroban requires map keys to be unique and in ascending order. The
    /// builder sorts the entries, so the order given here does not matter,
    /// and rejects a repeated key. Nesting is limited to [`MAX_ARG_DEPTH`].
    Map(Vec<(ScValInput, ScValInput)>),
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

fn conversion_error(kind: &'static str, reason: impl Into<String>) -> BuilderError {
    BuilderError::ArgumentConversionError {
        kind,
        reason: reason.into(),
    }
}

/// Decodes a hex string (digits only, even length).
fn decode_hex(digits: &str) -> Result<Vec<u8>, BuilderError> {
    if !digits.len().is_multiple_of(2) {
        return Err(conversion_error(
            "bytes",
            "hex string has an odd number of digits",
        ));
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    for pair in digits.as_bytes().chunks(2) {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        out.push(high << 4 | low);
    }
    Ok(out)
}

fn hex_value(byte: u8) -> Result<u8, BuilderError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(conversion_error(
            "bytes",
            format!("{:?} is not a hex digit", char::from(byte)),
        )),
    }
}

/// Decodes standard-alphabet base64 with canonical `=` padding.
fn decode_base64(text: &str) -> Result<Vec<u8>, BuilderError> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(conversion_error(
            "bytes",
            "base64 string length must be a multiple of 4 (use `=` padding)",
        ));
    }
    let padding = bytes.iter().rev().take_while(|b| **b == b'=').count();
    if padding > 2 {
        return Err(conversion_error(
            "bytes",
            "base64 string has too much padding",
        ));
    }
    let body = &bytes[..bytes.len() - padding];
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for &byte in body {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => {
                return Err(conversion_error(
                    "bytes",
                    format!("{:?} is not a base64 character", char::from(byte)),
                ))
            }
        };
        buffer = buffer << 6 | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    // Canonical form: the unused trailing bits must be zero.
    if buffer != 0 {
        return Err(conversion_error(
            "bytes",
            "base64 string has non-zero trailing bits",
        ));
    }
    Ok(out)
}

fn parse_bytes(value: &str) -> Result<ScBytes, BuilderError> {
    let decoded = match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(digits) => decode_hex(digits)?,
        None => decode_base64(value)?,
    };
    decoded
        .try_into()
        .map(ScBytes)
        .map_err(|e: stellar_xdr::Error| conversion_error("bytes", e.to_string()))
}

fn parse_address(value: &str) -> Result<ScAddress, BuilderError> {
    // A string starting with `S` may be a secret seed. It is never echoed,
    // whether or not it is valid: this project never handles secret keys.
    if value.starts_with('S') {
        return Err(conversion_error(
            "address",
            "a secret seed is not an address; expected a G... account or C... contract address (value not shown)",
        ));
    }
    match Strkey::from_string(value) {
        Ok(Strkey::PublicKeyEd25519(key)) => Ok(ScAddress::Account(AccountId(
            PublicKey::PublicKeyTypeEd25519(Uint256(key.0)),
        ))),
        Ok(Strkey::Contract(contract)) => Ok(ScAddress::Contract(ContractId(Hash(contract.0)))),
        Ok(_) => Err(conversion_error(
            "address",
            format!("{value:?} is not a G... account or C... contract address"),
        )),
        Err(error) => Err(conversion_error(
            "address",
            format!("{value:?} is not a valid address: {error}"),
        )),
    }
}

fn to_scval(input: &ScValInput) -> Result<ScVal, BuilderError> {
    to_scval_nested(input, 0)
}

/// Converts one argument; `depth` is how many `Vec`/`Map` containers
/// enclose it.
fn to_scval_nested(input: &ScValInput, depth: usize) -> Result<ScVal, BuilderError> {
    Ok(match input {
        ScValInput::Bytes(value) => ScVal::Bytes(parse_bytes(value)?),
        ScValInput::Address(value) => ScVal::Address(parse_address(value)?),
        ScValInput::Vec(items) => {
            if depth + 1 > MAX_ARG_DEPTH {
                return Err(BuilderError::RecursionDepthExceeded {
                    limit: MAX_ARG_DEPTH,
                });
            }
            let converted = items
                .iter()
                .map(|item| to_scval_nested(item, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            let vec = VecM::try_from(converted)
                .map_err(|e| BuilderError::InvalidArgument(e.to_string()))?;
            ScVal::Vec(Some(ScVec(vec)))
        }
        ScValInput::Map(entries) => {
            if depth + 1 > MAX_ARG_DEPTH {
                return Err(BuilderError::RecursionDepthExceeded {
                    limit: MAX_ARG_DEPTH,
                });
            }
            let mut converted = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                converted.push(ScMapEntry {
                    key: to_scval_nested(key, depth + 1)?,
                    val: to_scval_nested(value, depth + 1)?,
                });
            }
            converted.sort_by(|a, b| a.key.cmp(&b.key));
            if converted.windows(2).any(|pair| pair[0].key == pair[1].key) {
                return Err(BuilderError::DuplicateMapKey);
            }
            let map = VecM::try_from(converted)
                .map_err(|e| BuilderError::InvalidArgument(e.to_string()))?;
            ScVal::Map(Some(ScMap(map)))
        }
        _ => to_scalar(input)?,
    })
}

fn to_scalar(input: &ScValInput) -> Result<ScVal, BuilderError> {
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
        // Handled by `to_scval_nested`; not reachable through `to_scval`.
        ScValInput::Bytes(_) | ScValInput::Address(_) | ScValInput::Vec(_) | ScValInput::Map(_) => {
            return Err(BuilderError::InvalidArgument(
                "composite argument reached the scalar converter".to_string(),
            ))
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
