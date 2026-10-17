//! Lossless roundtrip coverage for TransactionEnvelope XDR.
//!
//! The property under test: for any of the three envelope variants (V0, V1,
//! FeeBump), `encode_transaction_envelope(decode_transaction_envelope(bytes))
//! == bytes` — byte for byte, with signatures and memos preserved. Negative
//! cases assert that corrupted base64 and truncated buffers fail with the
//! typed `XdrDecodeError` and a byte offset, rather than panicking.

use base64::Engine as _;

use canary_xdr::{decode, decode_transaction_envelope, encode_transaction_envelope, XdrTypeName};
use stellar_xdr::{
    Asset, BytesM, DecoratedSignature, FeeBumpTransaction, FeeBumpTransactionEnvelope,
    FeeBumpTransactionExt, Memo, MuxedAccount, Operation, OperationBody, PaymentOp, Preconditions,
    SequenceNumber, Signature, SignatureHint, StringM, TimeBounds, TimePoint, Transaction,
    TransactionEnvelope, TransactionExt, TransactionV0, TransactionV0Envelope, TransactionV0Ext,
    TransactionV1Envelope, Uint256, VecM,
};

fn b64_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64_decode(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .expect("test input is valid base64")
}

/// A distinct, deterministic signature per seed so a roundtrip that drops
/// or reorders signatures is detectable.
fn signature(seed: u8) -> DecoratedSignature {
    DecoratedSignature {
        hint: SignatureHint([seed; 4]),
        signature: Signature(BytesM::try_from(vec![seed; 64]).expect("64 bytes fits BytesM<64>")),
    }
}

fn account(seed: u8) -> MuxedAccount {
    MuxedAccount::Ed25519(Uint256([seed; 32]))
}

fn v0_envelope() -> TransactionEnvelope {
    TransactionEnvelope::TxV0(TransactionV0Envelope {
        tx: TransactionV0 {
            source_account_ed25519: Uint256([9; 32]),
            fee: 100,
            seq_num: SequenceNumber(42),
            time_bounds: Some(TimeBounds {
                min_time: TimePoint(0),
                max_time: TimePoint(1_735_689_600),
            }),
            memo: Memo::Text(StringM::try_from("canary v0 memo".to_string()).expect("fits 28")),
            operations: VecM::default(),
            ext: TransactionV0Ext::default(),
        },
        signatures: VecM::try_from(vec![signature(1), signature(2)]).expect("fits 20"),
    })
}

fn v1_transaction() -> Transaction {
    Transaction {
        source_account: account(3),
        fee: 300,
        seq_num: SequenceNumber(1_001),
        cond: Preconditions::Time(TimeBounds {
            min_time: TimePoint(0),
            max_time: TimePoint(1_735_689_600),
        }),
        memo: Memo::Id(987_654_321),
        operations: VecM::try_from(vec![Operation {
            source_account: None,
            body: OperationBody::Payment(PaymentOp {
                destination: MuxedAccount::Ed25519(Uint256([4; 32])),
                asset: Asset::Native,
                amount: 2_500_000_000,
            }),
        }])
        .expect("fits 100"),
        ext: TransactionExt::default(),
    }
}

fn v1_envelope() -> TransactionEnvelope {
    TransactionEnvelope::Tx(TransactionV1Envelope {
        tx: v1_transaction(),
        signatures: VecM::try_from(vec![signature(3)]).expect("fits 20"),
    })
}

fn fee_bump_envelope() -> TransactionEnvelope {
    TransactionEnvelope::TxFeeBump(FeeBumpTransactionEnvelope {
        tx: FeeBumpTransaction {
            fee_source: account(5),
            fee: 10_000_000,
            inner_tx: stellar_xdr::FeeBumpTransactionInnerTx::Tx(TransactionV1Envelope {
                tx: v1_transaction(),
                signatures: VecM::try_from(vec![signature(4)]).expect("fits 20"),
            }),
            ext: FeeBumpTransactionExt::default(),
        },
        signatures: VecM::try_from(vec![signature(5), signature(6)]).expect("fits 20"),
    })
}

/// The central property of this module: decode then encode reproduces the
/// input bytes exactly, for each of the three envelope variants.
fn assert_lossless_roundtrip(envelope: TransactionEnvelope) {
    let encoded = encode_transaction_envelope(&envelope).expect("encodes");
    let decoded = decode_transaction_envelope(&encoded).expect("decodes");
    let re_encoded = encode_transaction_envelope(&decoded).expect("re-encodes");

    assert_eq!(
        re_encoded, encoded,
        "encode(decode(bytes)) must equal bytes byte for byte"
    );
    assert_eq!(
        decoded, envelope,
        "the decoded envelope must equal the original value"
    );
}

#[test]
fn v0_envelope_roundtrips_byte_for_byte() {
    assert_lossless_roundtrip(v0_envelope());
}

#[test]
fn v1_envelope_roundtrips_byte_for_byte() {
    assert_lossless_roundtrip(v1_envelope());
}

#[test]
fn fee_bump_envelope_roundtrips_byte_for_byte() {
    assert_lossless_roundtrip(fee_bump_envelope());
}

/// The motivating failure mode from the issue: a roundtrip must not drop
/// signatures or the memo. Checked against the decoded value directly, not
/// only against the byte comparison.
#[test]
fn roundtrip_preserves_signatures_and_memo_fields() {
    let decoded =
        decode_transaction_envelope(&encode_transaction_envelope(&v0_envelope()).expect("encodes"))
            .expect("decodes");

    match decoded {
        TransactionEnvelope::TxV0(envelope) => {
            assert_eq!(
                envelope.tx.memo,
                Memo::Text(StringM::try_from("canary v0 memo".to_string()).expect("fits 28"))
            );
            assert_eq!(envelope.signatures.len(), 2);
            assert_eq!(envelope.signatures[0].hint, SignatureHint([1; 4]));
            assert_eq!(envelope.signatures[1].hint, SignatureHint([2; 4]));
            assert_eq!(
                envelope.signatures[0].signature,
                Signature(BytesM::try_from(vec![1; 64]).expect("fits 64"))
            );
        }
        other => panic!("expected a V0 envelope, got {other:?}"),
    }
}

/// The fee-bump roundtrip must keep the inner envelope and its signature,
/// not just the outer wrapper.
#[test]
fn fee_bump_roundtrip_preserves_the_inner_envelope() {
    let decoded = decode_transaction_envelope(
        &encode_transaction_envelope(&fee_bump_envelope()).expect("encodes"),
    )
    .expect("decodes");

    match decoded {
        TransactionEnvelope::TxFeeBump(envelope) => {
            assert_eq!(envelope.tx.fee, 10_000_000);
            assert_eq!(
                envelope.tx.inner_tx,
                stellar_xdr::FeeBumpTransactionInnerTx::Tx(TransactionV1Envelope {
                    tx: v1_transaction(),
                    signatures: VecM::try_from(vec![signature(4)]).expect("fits 20"),
                })
            );
            assert_eq!(envelope.signatures.len(), 2);
        }
        other => panic!("expected a FeeBump envelope, got {other:?}"),
    }
}

/// The fixture pipeline goes through `decode`/`encode` with a type name;
/// the typed entry points must agree with it so both paths roundtrip.
#[test]
fn the_typed_api_and_the_fixture_pipeline_agree() {
    for envelope in [v0_envelope(), v1_envelope(), fee_bump_envelope()] {
        let encoded = encode_transaction_envelope(&envelope).expect("encodes");

        let via_fixture_path = decode(XdrTypeName::TransactionEnvelope, &encoded)
            .expect("decodes through the generic path");
        let via_typed_path = decode_transaction_envelope(&encoded).expect("decodes");
        match via_fixture_path {
            canary_xdr::DecodedValue::TransactionEnvelope(v) => assert_eq!(*v, via_typed_path),
            other => panic!(
                "decode with XdrTypeName::TransactionEnvelope must hold an envelope, got {other:?}"
            ),
        }
    }
}

/// `from_xdr_base64` tolerates embedded whitespace; so must the typed
/// decoder, or the two would disagree on the same fixture input.
#[test]
fn decoding_tolerates_embedded_whitespace_like_from_xdr_base64() {
    let encoded = encode_transaction_envelope(&v1_envelope()).expect("encodes");
    let wrapped = encoded
        .as_bytes()
        .chunks(24)
        .map(|chunk| std::str::from_utf8(chunk).expect("ascii"))
        .collect::<Vec<_>>()
        .join("\n");

    let decoded = decode_transaction_envelope(&wrapped).expect("decodes");
    assert_eq!(
        encode_transaction_envelope(&decoded).expect("re-encodes"),
        encoded
    );
}

#[test]
fn corrupted_base64_returns_a_typed_error_at_offset_zero() {
    let err = decode_transaction_envelope("%%% not base64 %%%").unwrap_err();
    assert!(
        matches!(err, canary_xdr::XdrDecodeError::InvalidBase64(_)),
        "expected InvalidBase64, got {err:?}"
    );
    assert_eq!(err.byte_offset(), 0);
    assert!(err.to_string().contains("base64"), "{err}");
}

/// A valid envelope cut short must report `InvalidXdr` with the offset
/// inside the truncated buffer — the diagnostic the issue asks for.
#[test]
fn truncated_xdr_returns_a_typed_error_with_a_byte_offset() {
    let encoded = encode_transaction_envelope(&v1_envelope()).expect("encodes");
    let bytes = b64_decode(&encoded);
    let truncated = &bytes[..bytes.len() - 16];
    let truncated_b64 = b64_encode(truncated);

    let err = decode_transaction_envelope(&truncated_b64).unwrap_err();
    match &err {
        canary_xdr::XdrDecodeError::InvalidXdr {
            type_name,
            offset,
            message,
        } => {
            assert_eq!(*type_name, XdrTypeName::TransactionEnvelope);
            assert!(
                *offset > 0 && *offset <= truncated.len() as u64,
                "offset {offset} must point into the {}-byte truncated buffer",
                truncated.len()
            );
            assert!(!message.is_empty(), "the error must explain itself");
        }
        other => panic!("expected InvalidXdr, got {other:?}"),
    }
    assert!(err.to_string().contains("byte offset"), "{err}");
}

#[test]
fn trailing_bytes_after_a_complete_envelope_return_a_typed_error() {
    let encoded = encode_transaction_envelope(&v1_envelope()).expect("encodes");
    let mut bytes = b64_decode(&encoded);
    bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let err = decode_transaction_envelope(&b64_encode(&bytes)).unwrap_err();
    match &err {
        canary_xdr::XdrDecodeError::InvalidXdr { message, .. } => {
            assert!(message.contains("trailing"), "message: {message}");
        }
        other => panic!("expected InvalidXdr, got {other:?}"),
    }
}

/// Bytes that are valid base64 and a valid prefix, but describe a
/// different (corrupt) discriminant, must also be rejected without
/// panicking.
#[test]
fn an_invalid_envelope_discriminant_is_rejected_without_panicking() {
    // Discriminant 99 is not one of TxV0 (0), Tx (1), TxFeeBump (2).
    let corrupt = b64_encode(&[0, 0, 0, 99, 0, 0, 0, 0]);
    let err = decode_transaction_envelope(&corrupt).unwrap_err();
    assert!(
        matches!(err, canary_xdr::XdrDecodeError::InvalidXdr { .. }),
        "expected InvalidXdr, got {err:?}"
    );
}
