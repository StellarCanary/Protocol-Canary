//! Soroban argument builder tests: 128-bit, 256-bit, Void, Duration and
//! Timepoint (issue #488), Bytes and Address (issue #427), and Vec and Map
//! (issue #428) simulation arguments.
//!
//! Every encoding test round-trips through `TransactionEnvelope::from_xdr_base64`
//! so a passing assertion proves the argument survived as real XDR, not just
//! that a string was produced.

use canary_soroban::{
    build_invoke_transaction_envelope, BuilderError, InvocationSpec, ScValInput, MAX_ARG_DEPTH,
};
use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
use stellar_xdr::{
    AccountId, ContractId, Duration, Hash, HostFunction, Int128Parts, Int256Parts, Limits,
    OperationBody, PublicKey, ReadXdr, ScAddress, ScBytes, ScMap, ScMapEntry, ScVal, ScVec,
    TimePoint, TransactionEnvelope, UInt128Parts, UInt256Parts, Uint256,
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

fn account_strkey(byte: u8) -> String {
    StrkeyPublicKey([byte; 32]).to_string().to_string()
}

fn contract_strkey(byte: u8) -> String {
    StrkeyContract([byte; 32]).to_string().to_string()
}

fn conversion_message(error: BuilderError) -> String {
    match error {
        BuilderError::ArgumentConversionError { reason, .. } => reason,
        other => panic!("expected ArgumentConversionError, got {other:?}"),
    }
}

#[test]
fn encodes_an_account_address() {
    let values = encode(vec![ScValInput::Address(account_strkey(7))]);
    assert_eq!(
        values,
        vec![ScVal::Address(ScAddress::Account(AccountId(
            PublicKey::PublicKeyTypeEd25519(Uint256([7; 32]))
        )))]
    );
}

#[test]
fn encodes_a_contract_address() {
    let values = encode(vec![ScValInput::Address(contract_strkey(9))]);
    assert_eq!(
        values,
        vec![ScVal::Address(ScAddress::Contract(ContractId(Hash(
            [9; 32]
        ))))]
    );
}

#[test]
fn rejects_a_g_address_with_a_bad_checksum() {
    let mut address = account_strkey(7);
    let last = address.pop().unwrap();
    address.push(if last == 'A' { 'B' } else { 'A' });
    let message = conversion_message(build_err(vec![ScValInput::Address(address)]));
    assert!(message.contains("not a valid address"), "{message}");
}

#[test]
fn rejects_malformed_and_wrong_kind_addresses() {
    for bad in ["", "G", "not-an-address", "CABC", "gaaaa"] {
        let err = build_err(vec![ScValInput::Address(bad.to_string())]);
        assert!(
            matches!(err, BuilderError::ArgumentConversionError { .. }),
            "{bad:?}: {err:?}"
        );
    }
}

#[test]
fn a_secret_seed_is_rejected_and_never_echoed() {
    let seed = format!("S{}", "B".repeat(55));
    let err = build_err(vec![ScValInput::Address(seed.clone())]);
    let message = conversion_message(err);
    assert!(!message.contains(&seed));
    assert!(message.contains("secret seed"));
}

#[test]
fn encodes_hex_bytes_with_either_prefix_case() {
    let values = encode(vec![
        ScValInput::Bytes("0xdeadBEEF".to_string()),
        ScValInput::Bytes("0X00ff".to_string()),
        ScValInput::Bytes("0x".to_string()),
    ]);
    let bytes = |data: &[u8]| ScVal::Bytes(ScBytes(data.to_vec().try_into().unwrap()));
    assert_eq!(
        values,
        vec![
            bytes(&[0xde, 0xad, 0xbe, 0xef]),
            bytes(&[0x00, 0xff]),
            bytes(&[])
        ]
    );
}

#[test]
fn encodes_base64_bytes_and_distinguishes_hex_by_prefix() {
    let values = encode(vec![
        ScValInput::Bytes("3q2+7w==".to_string()),
        // Valid in both alphabets; without `0x` it is base64.
        ScValInput::Bytes("abcd".to_string()),
        ScValInput::Bytes(String::new()),
    ]);
    let bytes = |data: &[u8]| ScVal::Bytes(ScBytes(data.to_vec().try_into().unwrap()));
    assert_eq!(
        values,
        vec![
            bytes(&[0xde, 0xad, 0xbe, 0xef]),
            bytes(&[0x69, 0xb7, 0x1d]),
            bytes(&[]),
        ]
    );
}

#[test]
fn rejects_malformed_bytes() {
    for bad in [
        "0xzz",
        "0xabc",
        "3q2+7w=",
        "3q2+7w===",
        "3q2+7w==x",
        "3q2+7x==",
        "a b c d",
    ] {
        let err = build_err(vec![ScValInput::Bytes(bad.to_string())]);
        assert!(
            matches!(err, BuilderError::ArgumentConversionError { .. }),
            "{bad:?}: {err:?}"
        );
    }
}

#[test]
fn encodes_nested_vectors() {
    let values = encode(vec![ScValInput::Vec(vec![
        ScValInput::U32(1),
        ScValInput::Vec(vec![
            ScValInput::Bool(true),
            ScValInput::Symbol("x".to_string()),
        ]),
        ScValInput::Vec(vec![]),
    ])]);
    let ScVal::Vec(Some(ScVec(outer))) = &values[0] else {
        panic!("expected a vec");
    };
    assert_eq!(outer.len(), 3);
    assert_eq!(outer[0], ScVal::U32(1));
    let ScVal::Vec(Some(ScVec(inner))) = &outer[1] else {
        panic!("expected a nested vec");
    };
    assert_eq!(inner[0], ScVal::Bool(true));
    assert_eq!(
        outer[2],
        ScVal::Vec(Some(ScVec(vec![].try_into().unwrap())))
    );
}

#[test]
fn map_entries_are_sorted_by_key() {
    let values = encode(vec![ScValInput::Map(vec![
        (ScValInput::U32(3), ScValInput::Symbol("c".to_string())),
        (ScValInput::U32(1), ScValInput::Symbol("a".to_string())),
        (ScValInput::U32(2), ScValInput::Symbol("b".to_string())),
    ])]);
    let ScVal::Map(Some(ScMap(entries))) = &values[0] else {
        panic!("expected a map");
    };
    let keys: Vec<&ScVal> = entries.iter().map(|ScMapEntry { key, .. }| key).collect();
    assert_eq!(keys, [&ScVal::U32(1), &ScVal::U32(2), &ScVal::U32(3)]);
}

#[test]
fn input_order_does_not_change_the_encoding() {
    let one = vec![
        (ScValInput::Symbol("b".to_string()), ScValInput::U32(2)),
        (ScValInput::Symbol("a".to_string()), ScValInput::U32(1)),
    ];
    let mut two = one.clone();
    two.reverse();
    assert_eq!(
        encode(vec![ScValInput::Map(one)]),
        encode(vec![ScValInput::Map(two)])
    );
}

#[test]
fn duplicate_map_keys_are_rejected() {
    let err = build_err(vec![ScValInput::Map(vec![
        (ScValInput::U32(1), ScValInput::U32(1)),
        (ScValInput::U32(1), ScValInput::U32(2)),
    ])]);
    assert!(matches!(err, BuilderError::DuplicateMapKey), "{err:?}");
}

#[test]
fn maps_may_hold_vectors_and_maps() {
    let values = encode(vec![ScValInput::Map(vec![(
        ScValInput::Symbol("list".to_string()),
        ScValInput::Vec(vec![ScValInput::Map(vec![(
            ScValInput::U32(1),
            ScValInput::Address(contract_strkey(1)),
        )])]),
    )])]);
    assert!(matches!(values[0], ScVal::Map(Some(_))));
}

fn nested_vec(levels: usize) -> ScValInput {
    let mut value = ScValInput::U32(0);
    for _ in 0..levels {
        value = ScValInput::Vec(vec![value]);
    }
    value
}

#[test]
fn depth_of_exactly_the_limit_is_accepted() {
    assert_eq!(encode(vec![nested_vec(MAX_ARG_DEPTH)]).len(), 1);
}

#[test]
fn depth_over_the_limit_is_rejected_for_vec_and_map() {
    let err = build_err(vec![nested_vec(MAX_ARG_DEPTH + 1)]);
    assert!(
        matches!(err, BuilderError::RecursionDepthExceeded { limit: 8 }),
        "{err:?}"
    );

    let mut map = ScValInput::U32(0);
    for _ in 0..=MAX_ARG_DEPTH {
        map = ScValInput::Map(vec![(ScValInput::U32(0), map)]);
    }
    assert!(matches!(
        build_err(vec![map]),
        BuilderError::RecursionDepthExceeded { .. }
    ));
}

#[test]
fn a_deeply_nested_input_does_not_overflow_the_stack() {
    // Far past the limit: the depth check must stop the recursion early.
    let err = build_err(vec![nested_vec(50)]);
    assert!(matches!(err, BuilderError::RecursionDepthExceeded { .. }));
}
