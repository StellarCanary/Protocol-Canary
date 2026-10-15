//! Soroban argument builder tests (issue #488): 128-bit, 256-bit, Void,
//! Duration, and Timepoint simulation arguments.
//!
//! Every encoding test round-trips through `TransactionEnvelope::from_xdr_base64`
//! so a passing assertion proves the argument survived as real XDR, not just
//! that a string was produced.

use canary_soroban::{build_invoke_transaction_envelope, BuilderError, InvocationSpec, ScValInput};
use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
use stellar_xdr::{
    Duration, HostFunction, Int128Parts, Int256Parts, Limits, OperationBody, ReadXdr, ScVal,
    TimePoint, TransactionEnvelope, UInt128Parts, UInt256Parts,
};

/// A well-formed but arbitrary account/contract pair (all-zero payload bytes)
/// that only needs to satisfy strkey decoding; nothing here touches a network.
fn spec_with(args: Vec<ScValInput>) -> InvocationSpec {
    InvocationSpec {
        // `Strkey*::to_string` returns a heapless string; the second
        // `to_string` converts it to a standard `String`.
        source_account: StrkeyPublicKey([0u8; 32]).to_string().to_string(),
        contract_id: StrkeyContract([0u8; 32]).to_string().to_string(),
        function_name: "hello".to_string(),
        args,
        sequence_number: 1,
    }
}

/// Builds the envelope for `args`, decodes it back from base64 XDR, and
/// returns the invocation's `ScVal` arguments.
fn encode(args: Vec<ScValInput>) -> Vec<ScVal> {
    let base64 = build_invoke_transaction_envelope(&spec_with(args)).expect("builds");
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
    invoke_args.args.to_vec()
}

/// Builds an envelope for `args` and returns the builder error.
fn build_err(args: Vec<ScValInput>) -> BuilderError {
    build_invoke_transaction_envelope(&spec_with(args)).unwrap_err()
}

#[test]
fn encodes_128_bit_boundaries_and_hex_forms() {
    let values = encode(vec![
        // i128::MAX and i128::MIN in decimal, plus -1 and their hex forms.
        ScValInput::I128("170141183460469231731687303715884105727".to_string()),
        ScValInput::I128("-170141183460469231731687303715884105728".to_string()),
        ScValInput::I128("-1".to_string()),
        ScValInput::I128("0x7fffffffffffffffffffffffffffffff".to_string()),
        ScValInput::I128("-0x80000000000000000000000000000000".to_string()),
        // u128::MAX, zero, and hex forms (lower- and upper-case prefix).
        ScValInput::U128("340282366920938463463374607431768211455".to_string()),
        ScValInput::U128("0".to_string()),
        ScValInput::U128("0xffffffffffffffffffffffffffffffff".to_string()),
        ScValInput::U128("0XFF".to_string()),
    ]);

    assert_eq!(
        values,
        vec![
            ScVal::I128(Int128Parts {
                hi: i64::MAX,
                lo: u64::MAX,
            }),
            ScVal::I128(Int128Parts {
                hi: i64::MIN,
                lo: 0,
            }),
            ScVal::I128(Int128Parts {
                hi: -1,
                lo: u64::MAX
            }),
            ScVal::I128(Int128Parts {
                hi: i64::MAX,
                lo: u64::MAX,
            }),
            ScVal::I128(Int128Parts {
                hi: i64::MIN,
                lo: 0,
            }),
            ScVal::U128(UInt128Parts {
                hi: u64::MAX,
                lo: u64::MAX,
            }),
            ScVal::U128(UInt128Parts { hi: 0, lo: 0 }),
            ScVal::U128(UInt128Parts {
                hi: u64::MAX,
                lo: u64::MAX,
            }),
            ScVal::U128(UInt128Parts { hi: 0, lo: 0xff }),
        ]
    );
}

#[test]
fn encodes_128_bit_values_without_loss_of_precision() {
    let values = encode(vec![
        ScValInput::I128("123456789012345678901234567890".to_string()),
        ScValInput::I128("-123456789012345678901234567890".to_string()),
        ScValInput::U128("340282366920938463463374607431768211454".to_string()),
    ]);

    // Reconstruct each value from its two 64-bit halves: the high half is an
    // arithmetic shift, so this is an independent check of the split.
    let ScVal::I128(first) = &values[0] else {
        panic!("expected an i128");
    };
    let reconstructed = ((first.hi as i128) << 64) | (first.lo as i128);
    assert_eq!(reconstructed, 123456789012345678901234567890_i128);

    let ScVal::I128(second) = &values[1] else {
        panic!("expected an i128");
    };
    let reconstructed = ((second.hi as i128) << 64) | (second.lo as i128);
    assert_eq!(reconstructed, -123456789012345678901234567890_i128);

    let ScVal::U128(third) = &values[2] else {
        panic!("expected a u128");
    };
    let reconstructed = ((third.hi as u128) << 64) | u128::from(third.lo);
    assert_eq!(reconstructed, u128::MAX - 1);
}

#[test]
fn encodes_256_bit_boundaries_and_hex_forms() {
    let values = encode(vec![
        // u256::MAX (2^256 - 1) in decimal and hex.
        ScValInput::U256(
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
                .to_string(),
        ),
        ScValInput::U256(format!("0x{}", "f".repeat(64))),
        ScValInput::U256("0".to_string()),
        // i256::MAX (2^255 - 1) and i256::MIN (-2^255) in decimal and hex.
        ScValInput::I256(
            "57896044618658097711785492504343953926634992332820282019728792003956564819967"
                .to_string(),
        ),
        ScValInput::I256(
            "-57896044618658097711785492504343953926634992332820282019728792003956564819968"
                .to_string(),
        ),
        ScValInput::I256(format!("-0x8{}", "0".repeat(63))),
        ScValInput::I256("-1".to_string()),
    ]);

    assert_eq!(
        values,
        vec![
            ScVal::U256(UInt256Parts {
                hi_hi: u64::MAX,
                hi_lo: u64::MAX,
                lo_hi: u64::MAX,
                lo_lo: u64::MAX,
            }),
            ScVal::U256(UInt256Parts {
                hi_hi: u64::MAX,
                hi_lo: u64::MAX,
                lo_hi: u64::MAX,
                lo_lo: u64::MAX,
            }),
            ScVal::U256(UInt256Parts {
                hi_hi: 0,
                hi_lo: 0,
                lo_hi: 0,
                lo_lo: 0,
            }),
            ScVal::I256(Int256Parts {
                hi_hi: i64::MAX,
                hi_lo: u64::MAX,
                lo_hi: u64::MAX,
                lo_lo: u64::MAX,
            }),
            ScVal::I256(Int256Parts {
                hi_hi: i64::MIN,
                hi_lo: 0,
                lo_hi: 0,
                lo_lo: 0,
            }),
            ScVal::I256(Int256Parts {
                hi_hi: i64::MIN,
                hi_lo: 0,
                lo_hi: 0,
                lo_lo: 0,
            }),
            ScVal::I256(Int256Parts {
                hi_hi: -1,
                hi_lo: u64::MAX,
                lo_hi: u64::MAX,
                lo_lo: u64::MAX,
            }),
        ]
    );
}

#[test]
fn encodes_256_bit_values_across_word_boundaries() {
    let values = encode(vec![
        // 2^96 + 1: crosses from the lowest word into the third word.
        ScValInput::U256("79228162514264337593543950337".to_string()),
        // 2^128 + 5: lands in the high word pair.
        ScValInput::I256("340282366920938463463374607431768211461".to_string()),
        // Hex crossing a word boundary: 2^64 + 1.
        ScValInput::U256("0x10000000000000001".to_string()),
    ]);

    assert_eq!(
        values,
        vec![
            ScVal::U256(UInt256Parts {
                hi_hi: 0,
                hi_lo: 0,
                lo_hi: 1 << 32,
                lo_lo: 1,
            }),
            ScVal::I256(Int256Parts {
                hi_hi: 0,
                hi_lo: 1,
                lo_hi: 0,
                lo_lo: 5,
            }),
            ScVal::U256(UInt256Parts {
                hi_hi: 0,
                hi_lo: 0,
                lo_hi: 1,
                lo_lo: 1,
            }),
        ]
    );
}

#[test]
fn encodes_void_duration_and_timepoint_arguments() {
    let values = encode(vec![
        ScValInput::Void,
        ScValInput::Duration(0),
        ScValInput::Duration(300),
        ScValInput::Duration(u64::MAX),
        ScValInput::Timepoint(1_700_000_000),
        ScValInput::Timepoint(u64::MAX),
    ]);

    assert_eq!(
        values,
        vec![
            ScVal::Void,
            ScVal::Duration(Duration(0)),
            ScVal::Duration(Duration(300)),
            ScVal::Duration(Duration(u64::MAX)),
            ScVal::Timepoint(TimePoint(1_700_000_000)),
            ScVal::Timepoint(TimePoint(u64::MAX)),
        ]
    );
}

#[test]
fn encodes_a_single_envelope_mixing_every_new_argument_type() {
    let values = encode(vec![
        ScValInput::I128("-42".to_string()),
        ScValInput::U128("42".to_string()),
        ScValInput::I256("-42".to_string()),
        ScValInput::U256("42".to_string()),
        ScValInput::Void,
        ScValInput::Duration(60),
        ScValInput::Timepoint(1_700_000_000),
    ]);

    assert_eq!(
        values,
        vec![
            ScVal::I128(Int128Parts {
                hi: -1,
                lo: u64::MAX - 41
            }),
            ScVal::U128(UInt128Parts { hi: 0, lo: 42 }),
            ScVal::I256(Int256Parts {
                hi_hi: -1,
                hi_lo: u64::MAX,
                lo_hi: u64::MAX,
                lo_lo: u64::MAX - 41,
            }),
            ScVal::U256(UInt256Parts {
                hi_hi: 0,
                hi_lo: 0,
                lo_hi: 0,
                lo_lo: 42,
            }),
            ScVal::Void,
            ScVal::Duration(Duration(60)),
            ScVal::Timepoint(TimePoint(1_700_000_000)),
        ]
    );
}

#[test]
fn rejects_256_bit_strings_exceeding_the_integer_bounds_with_integer_overflow() {
    // 2^256 in decimal: one past u256::MAX.
    let err = build_err(vec![ScValInput::U256(
        "115792089237316195423570985008687907853269984665640564039457584007913129639936"
            .to_string(),
    )]);
    match err {
        BuilderError::IntegerOverflowError { value, bits } => {
            assert_eq!(
                value,
                "115792089237316195423570985008687907853269984665640564039457584007913129639936"
            );
            assert_eq!(bits, 256);
        }
        other => panic!("expected IntegerOverflowError, got {other:?}"),
    }

    // The same boundary in hex (65 hex digits = 260 bits).
    let err = build_err(vec![ScValInput::U256(format!("0x1{}", "0".repeat(64)))]);
    assert!(matches!(
        err,
        BuilderError::IntegerOverflowError { bits: 256, .. }
    ));

    // Positive i256 above i256::MAX (2^255).
    let err = build_err(vec![ScValInput::I256(
        "57896044618658097711785492504343953926634992332820282019728792003956564819968".to_string(),
    )]);
    assert!(matches!(
        err,
        BuilderError::IntegerOverflowError { bits: 256, .. }
    ));

    // Negative i256 below i256::MIN (-2^255 - 1).
    let err = build_err(vec![ScValInput::I256(
        "-57896044618658097711785492504343953926634992332820282019728792003956564819969"
            .to_string(),
    )]);
    assert!(matches!(
        err,
        BuilderError::IntegerOverflowError { bits: 256, .. }
    ));

    // A decimal magnitude too wide to accumulate into 256 bits at all.
    let err = build_err(vec![ScValInput::U256("9".repeat(78))]);
    assert!(matches!(
        err,
        BuilderError::IntegerOverflowError { bits: 256, .. }
    ));
}

#[test]
fn rejects_128_bit_strings_exceeding_the_integer_bounds_with_integer_overflow() {
    // 2^127 as i128: one past i128::MAX.
    let err = build_err(vec![ScValInput::I128(
        "170141183460469231731687303715884105728".to_string(),
    )]);
    assert!(matches!(
        err,
        BuilderError::IntegerOverflowError { bits: 128, .. }
    ));

    // -(2^127) - 1: one below i128::MIN.
    let err = build_err(vec![ScValInput::I128(
        "-170141183460469231731687303715884105729".to_string(),
    )]);
    assert!(matches!(
        err,
        BuilderError::IntegerOverflowError { bits: 128, .. }
    ));

    // 2^128 as u128: one past u128::MAX.
    let err = build_err(vec![ScValInput::U128(
        "340282366920938463463374607431768211456".to_string(),
    )]);
    assert!(matches!(
        err,
        BuilderError::IntegerOverflowError { bits: 128, .. }
    ));

    // A negative magnitude wider than u128 can hold at all.
    let err = build_err(vec![ScValInput::I128(
        "-9999999999999999999999999999999999999999".to_string(),
    )]);
    assert!(matches!(
        err,
        BuilderError::IntegerOverflowError { bits: 128, .. }
    ));
}

#[test]
fn rejects_malformed_integer_strings_with_invalid_argument() {
    for bad in ["", "abc", "12.5", "0x", "0xgg", "1x9", "0b101"] {
        let err = build_err(vec![ScValInput::U256(bad.to_string())]);
        match &err {
            BuilderError::InvalidArgument(message) => {
                assert!(message.contains(bad), "message {message:?} names {bad:?}");
            }
            other => panic!("{bad:?} should be InvalidArgument, got {other:?}"),
        }
    }

    for bad in ["", "abc", "12.5", "0x", "0xgg"] {
        let err = build_err(vec![ScValInput::I128(bad.to_string())]);
        assert!(
            matches!(err, BuilderError::InvalidArgument(_)),
            "{bad:?} should be InvalidArgument, got {err:?}"
        );
    }
}

#[test]
fn rejects_negative_values_for_unsigned_integer_kinds() {
    let err = build_err(vec![ScValInput::U128("-1".to_string())]);
    match &err {
        BuilderError::InvalidArgument(message) => assert!(message.contains("must not be negative")),
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    let err = build_err(vec![ScValInput::U256("-1".to_string())]);
    match &err {
        BuilderError::InvalidArgument(message) => assert!(message.contains("must not be negative")),
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
