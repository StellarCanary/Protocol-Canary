//! Decoder and encoder error paths (`[E-112]`).
//!
//! The decoder is the trust boundary for fixture payloads and for base64
//! XDR copied out of an RPC response, so every malformed input — not base64
//! at all, invalid padding, an empty string, a truncated buffer, a corrupt
//! discriminant, or a value followed by trailing bytes — must return the
//! typed [`XdrDecodeError`] rather than panicking or returning a partial
//! value.
//!
//! Encoder failures are covered from the other direction: `canary-xdr`
//! always encodes with `stellar_xdr::Limits::none()`, so a value produced by
//! the decoder can never fail to encode. The tests below assert that every
//! [`DecodedValue`] variant and both typed encoders are total and
//! deterministic, which is the encoder's error contract as this crate
//! exposes it.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

use canary_xdr::{
    decode, decode_ledger_entry, decode_transaction_envelope, encode, encode_ledger_entry,
    encode_transaction_envelope, DecodedValue, XdrDecodeError, XdrTypeName,
};
use stellar_xdr::{
    ContractExecutable, Hash, LedgerEntry, Limits, ScVal, StellarValue, TransactionEnvelope,
    WriteXdr,
};

fn to_base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

fn from_base64(text: &str) -> Vec<u8> {
    STANDARD.decode(text).expect("test input is valid base64")
}

fn envelope_xdr() -> String {
    TransactionEnvelope::default()
        .to_xdr_base64(Limits::none())
        .expect("the default envelope encodes")
}

fn ledger_entry_xdr() -> String {
    LedgerEntry::default()
        .to_xdr_base64(Limits::none())
        .expect("the default ledger entry encodes")
}

/// A default `StellarValue`: a small XDR value whose bytes are easy to
/// truncate deterministically.
fn stellar_value_bytes() -> Vec<u8> {
    StellarValue::default()
        .to_xdr(Limits::none())
        .expect("the default StellarValue encodes")
}

/// Asserts `err` is `InvalidXdr` for `expected_type_name` and returns the
/// offset it reports, checking the field and
/// [`XdrDecodeError::byte_offset`] agree.
fn assert_invalid_xdr(err: XdrDecodeError, expected_type_name: XdrTypeName) -> u64 {
    let byte_offset = err.byte_offset();
    match err {
        XdrDecodeError::InvalidXdr {
            type_name, offset, ..
        } => {
            assert_eq!(
                type_name, expected_type_name,
                "the error must name the type that was decoded"
            );
            assert_eq!(byte_offset, offset, "the offset is reported consistently");
            offset
        }
        XdrDecodeError::InvalidBase64(inner) => {
            panic!("expected InvalidXdr for {expected_type_name}, got InvalidBase64({inner})")
        }
    }
}

#[test]
fn every_supported_type_name_displays_and_parses_back() {
    for type_name in [
        XdrTypeName::StellarValue,
        XdrTypeName::ContractExecutable,
        XdrTypeName::ScVal,
        XdrTypeName::TransactionEnvelope,
        XdrTypeName::LedgerEntry,
    ] {
        let rendered = type_name.to_string();
        assert!(!rendered.is_empty());
        assert_eq!(rendered.parse::<XdrTypeName>(), Ok(type_name));
    }
}

#[test]
fn an_unknown_type_name_is_rejected_with_the_supported_list() {
    let err = "NotAType".parse::<XdrTypeName>().unwrap_err();
    assert!(err.contains("NotAType"), "{err}");
    for supported in [
        "StellarValue",
        "ContractExecutable",
        "ScVal",
        "TransactionEnvelope",
        "LedgerEntry",
    ] {
        assert!(err.contains(supported), "{err} should list {supported}");
    }
}

#[test]
fn non_base64_input_is_a_typed_error_with_no_byte_offset() {
    for input in [
        "not base64!",
        "AAAA AAAA!!",
        "@@@@@@@@",
        "AAAA\u{0}AAAA",
        // Standard alphabet only: URL-safe characters are not accepted.
        "AA-_AA==",
    ] {
        for err in [
            decode_transaction_envelope(input).unwrap_err(),
            decode_ledger_entry(input).unwrap_err(),
        ] {
            assert!(
                matches!(err, XdrDecodeError::InvalidBase64(_)),
                "{input:?} must be InvalidBase64, got {err:?}"
            );
            // There are no decoded bytes, so there is no offset to report.
            assert_eq!(err.byte_offset(), 0);
            assert!(err.to_string().contains("not valid base64"), "{err}");
        }
    }
}

#[test]
fn invalid_base64_padding_is_a_typed_error() {
    // Length not a multiple of four, a stray `=` in the middle, and a
    // truncated final quantum are all padding failures, not XDR failures.
    for input in ["A", "AAAAA", "AA=A", "AB=", "AAA==="] {
        let err = decode_transaction_envelope(input).unwrap_err();
        assert!(
            matches!(err, XdrDecodeError::InvalidBase64(_)),
            "{input:?} must be InvalidBase64, got {err:?}"
        );
        let err = decode_ledger_entry(input).unwrap_err();
        assert!(
            matches!(err, XdrDecodeError::InvalidBase64(_)),
            "{input:?} must be InvalidBase64, got {err:?}"
        );
    }
}

#[test]
fn a_zero_byte_payload_is_a_typed_error_not_a_panic() {
    // The empty string is valid base64 that decodes to zero bytes, so the
    // failure belongs to the XDR reader: it stops immediately at offset 0.
    for input in ["", "   ", "\n\t "] {
        let err = decode_transaction_envelope(input).unwrap_err();
        assert_eq!(assert_invalid_xdr(err, XdrTypeName::TransactionEnvelope), 0);

        let err = decode_ledger_entry(input).unwrap_err();
        assert_eq!(assert_invalid_xdr(err, XdrTypeName::LedgerEntry), 0);
    }
}

#[test]
fn a_truncated_payload_is_rejected_and_reports_where_the_reader_stopped() {
    let bytes = stellar_value_bytes();
    assert!(
        bytes.len() > 8,
        "the sample must be long enough to truncate"
    );

    // The generic decoder returns the official `stellar_xdr` error; what
    // matters is that a short buffer is an error, never a default value.
    for keep in [0, 1, bytes.len() / 2, bytes.len() - 1] {
        let truncated = &bytes[..keep];
        let err = decode(XdrTypeName::StellarValue, &to_base64(truncated));
        assert!(err.is_err(), "truncating to {keep} bytes must fail");
    }

    // The typed entry points report the byte offset they reached.
    let envelope_bytes = from_base64(&envelope_xdr());
    let truncated = &envelope_bytes[..envelope_bytes.len() - 1];
    let err = decode_transaction_envelope(&to_base64(truncated)).unwrap_err();
    let offset = assert_invalid_xdr(err, XdrTypeName::TransactionEnvelope);
    assert!(
        offset <= truncated.len() as u64,
        "offset {offset} must be inside the {} byte buffer",
        truncated.len()
    );

    let entry_bytes = from_base64(&ledger_entry_xdr());
    let truncated = &entry_bytes[..entry_bytes.len() / 2];
    let err = decode_ledger_entry(&to_base64(truncated)).unwrap_err();
    let offset = assert_invalid_xdr(err, XdrTypeName::LedgerEntry);
    assert!(
        offset <= truncated.len() as u64,
        "offset {offset} must be inside the {} byte buffer",
        truncated.len()
    );
}

#[test]
fn trailing_bytes_after_a_complete_value_are_rejected() {
    let envelope_bytes = from_base64(&envelope_xdr());
    for extra in [1usize, 4, 64] {
        let mut oversized = envelope_bytes.clone();
        oversized.extend(std::iter::repeat_n(0u8, extra));
        let err = decode_transaction_envelope(&to_base64(&oversized)).unwrap_err();
        let message = err_message(&err);
        let offset = assert_invalid_xdr(err, XdrTypeName::TransactionEnvelope);
        assert_eq!(
            offset,
            envelope_bytes.len() as u64,
            "a {extra} byte suffix must be reported at the end of the value"
        );
        assert!(
            message.contains("trailing byte(s)"),
            "the failure names the trailing bytes: {message}"
        );
    }

    let entry_bytes = from_base64(&ledger_entry_xdr());
    let mut oversized = entry_bytes.clone();
    oversized.extend(std::iter::repeat_n(0u8, 4));
    let err = decode_ledger_entry(&to_base64(&oversized)).unwrap_err();
    assert_eq!(
        assert_invalid_xdr(err, XdrTypeName::LedgerEntry),
        entry_bytes.len() as u64
    );
}

fn err_message(err: &XdrDecodeError) -> String {
    match err {
        XdrDecodeError::InvalidXdr { message, .. } => message.clone(),
        XdrDecodeError::InvalidBase64(_) => String::new(),
    }
}

#[test]
fn corrupted_bytes_are_rejected() {
    // An envelope type discriminant that no version defines.
    let envelope_bytes = from_base64(&envelope_xdr());
    let mut corrupted = envelope_bytes.clone();
    corrupted[..4].copy_from_slice(&5u32.to_be_bytes());
    let err = decode_transaction_envelope(&to_base64(&corrupted)).unwrap_err();
    assert!(matches!(err, XdrDecodeError::InvalidXdr { .. }), "{err:?}");

    // All-ones bytes: every discriminant and length prefix is absurd, so
    // the reader must fail rather than allocate from them.
    let ones = vec![0xff_u8; 128];
    for err in [
        decode_transaction_envelope(&to_base64(&ones)).unwrap_err(),
        decode_ledger_entry(&to_base64(&ones)).unwrap_err(),
    ] {
        assert!(matches!(err, XdrDecodeError::InvalidXdr { .. }), "{err:?}");
    }
}

#[test]
fn an_oversized_payload_is_rejected_without_panicking() {
    // 1 MiB of zeros: valid base64 whose decoded bytes are far too large to
    // be a ledger header or an envelope. The prefix happens to be a default
    // value, so the tail is reported as trailing bytes; either way the
    // result is a typed failure, not a default value and not a panic.
    let oversized = to_base64(&vec![0u8; 1024 * 1024]);
    for err in [
        decode_transaction_envelope(&oversized).unwrap_err(),
        decode_ledger_entry(&oversized).unwrap_err(),
    ] {
        assert!(matches!(err, XdrDecodeError::InvalidXdr { .. }), "{err:?}");
        assert!(err.byte_offset() > 0);
    }

    // A near-valid value with a large amount of garbage after it.
    let mut nearly_valid = from_base64(&ledger_entry_xdr());
    nearly_valid.extend(std::iter::repeat_n(0xaa_u8, 64 * 1024));
    let err = decode_ledger_entry(&to_base64(&nearly_valid)).unwrap_err();
    assert!(matches!(err, XdrDecodeError::InvalidXdr { .. }), "{err:?}");
}

#[test]
fn the_generic_decoder_also_fails_cleanly_on_invalid_input() {
    for input in ["", "not base64!", "AAAAA"] {
        let err = decode(XdrTypeName::ScVal, input).unwrap_err();
        assert!(
            !err.to_string().is_empty(),
            "{input:?} must produce a message"
        );
    }
    let err = decode(XdrTypeName::ContractExecutable, &to_base64(&[0xff; 16])).unwrap_err();
    assert!(!err.to_string().is_empty());
}

#[test]
fn encoding_every_decoded_variant_is_total() {
    let variants = [
        DecodedValue::StellarValue(StellarValue::default()),
        DecodedValue::ContractExecutable(ContractExecutable::Wasm(Hash([2; 32]))),
        DecodedValue::ScVal(ScVal::Bool(true)),
        DecodedValue::TransactionEnvelope(Box::default()),
        DecodedValue::LedgerEntry(Box::default()),
    ];

    for value in &variants {
        let type_name = match value {
            DecodedValue::StellarValue(_) => XdrTypeName::StellarValue,
            DecodedValue::ContractExecutable(_) => XdrTypeName::ContractExecutable,
            DecodedValue::ScVal(_) => XdrTypeName::ScVal,
            DecodedValue::TransactionEnvelope(_) => XdrTypeName::TransactionEnvelope,
            DecodedValue::LedgerEntry(_) => XdrTypeName::LedgerEntry,
        };

        let encoded = encode(value).expect("encoding a decoded value cannot fail");
        assert!(!encoded.is_empty());

        let decoded = decode(type_name, &encoded).expect("the encoded bytes decode again");
        assert_eq!(
            encode(&decoded).expect("re-encoding cannot fail"),
            encoded,
            "encoding is deterministic"
        );
    }
}

#[test]
fn the_typed_encoders_are_total_and_deterministic() {
    let envelope = TransactionEnvelope::default();
    let first = encode_transaction_envelope(&envelope).expect("envelopes always encode");
    let second = encode_transaction_envelope(&envelope).expect("envelopes always encode");
    assert_eq!(first, second);

    let entry = LedgerEntry::default();
    let first = encode_ledger_entry(&entry).expect("ledger entries always encode");
    let second = encode_ledger_entry(&entry).expect("ledger entries always encode");
    assert_eq!(first, second);

    // The generic and typed paths agree, so a caller cannot get different
    // bytes depending on which API it used.
    assert_eq!(
        encode(&DecodedValue::TransactionEnvelope(Box::new(
            envelope.clone()
        )))
        .unwrap(),
        encode_transaction_envelope(&envelope).unwrap()
    );
    assert_eq!(
        encode(&DecodedValue::LedgerEntry(Box::new(entry.clone()))).unwrap(),
        encode_ledger_entry(&entry).unwrap()
    );
}
