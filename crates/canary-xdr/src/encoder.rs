//! Re-encoding a previously decoded XDR value.

use stellar_xdr::{
    LedgerCloseMeta, LedgerEntry, LedgerHeader, Limits, TransactionEnvelope, WriteXdr,
};

use crate::decoder::DecodedValue;

/// Re-encodes `value` back to base64 XDR.
pub fn encode(value: &DecodedValue) -> Result<String, stellar_xdr::Error> {
    match value {
        DecodedValue::StellarValue(v) => v.to_xdr_base64(Limits::none()),
        DecodedValue::ContractExecutable(v) => v.to_xdr_base64(Limits::none()),
        DecodedValue::ScVal(v) => v.to_xdr_base64(Limits::none()),
        DecodedValue::TransactionEnvelope(v) => v.to_xdr_base64(Limits::none()),
        DecodedValue::LedgerEntry(v) => v.to_xdr_base64(Limits::none()),
        DecodedValue::LedgerHeader(v) => v.to_xdr_base64(Limits::none()),
        DecodedValue::LedgerCloseMeta(v) => v.to_xdr_base64(Limits::none()),
    }
}

/// Encodes `envelope` (V0, V1, or FeeBump) as canonical base64 XDR.
///
/// `stellar-xdr`'s XDR writer is deterministic, so for any envelope
/// produced by [`crate::decode_transaction_envelope`],
/// `encode_transaction_envelope(decode_transaction_envelope(bytes))`
/// reproduces `bytes` exactly — signatures, memos, and every other field
/// included.
pub fn encode_transaction_envelope(
    envelope: &TransactionEnvelope,
) -> Result<String, stellar_xdr::Error> {
    envelope.to_xdr_base64(Limits::none())
}

/// Encodes `entry` as canonical base64 XDR.
///
/// `stellar-xdr`'s XDR writer is deterministic, so for any entry produced
/// by [`crate::decode_ledger_entry`],
/// `encode_ledger_entry(decode_ledger_entry(bytes))` reproduces `bytes`
/// exactly — `lastModifiedLedgerSeq`, extension data (including a
/// `ContractCode` entry's V1 cost inputs), and every other field included.
pub fn encode_ledger_entry(entry: &LedgerEntry) -> Result<String, stellar_xdr::Error> {
    entry.to_xdr_base64(Limits::none())
}

/// Encodes `header` as canonical base64 XDR.
///
/// `stellar-xdr`'s XDR writer is deterministic, so for any header produced
/// by [`crate::decode_ledger_header`],
/// `encode_ledger_header(decode_ledger_header(bytes))` reproduces `bytes`
/// exactly.
pub fn encode_ledger_header(header: &LedgerHeader) -> Result<String, stellar_xdr::Error> {
    header.to_xdr_base64(Limits::none())
}

/// Encodes `meta` (V0, V1, or V2) as canonical base64 XDR.
///
/// `stellar-xdr`'s XDR writer is deterministic, so for any metadata produced
/// by [`crate::decode_ledger_close_meta`],
/// `encode_ledger_close_meta(decode_ledger_close_meta(bytes))` reproduces `bytes`
/// exactly.
pub fn encode_ledger_close_meta(meta: &LedgerCloseMeta) -> Result<String, stellar_xdr::Error> {
    meta.to_xdr_base64(Limits::none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{
        decode, decode_ledger_close_meta, decode_ledger_entry, decode_ledger_header,
        decode_transaction_envelope, DecodedValue, XdrTypeName,
    };
    use stellar_xdr::{
        ContractExecutable, ContractId, Hash, MuxedContract, MuxedEd25519Account, ScAddress, ScVal,
        StellarValue, Uint256,
    };

    #[test]
    fn encoding_a_decoded_value_reproduces_the_same_value() {
        let value = StellarValue::default();
        let base64 = encode(&DecodedValue::StellarValue(value.clone())).expect("encodes");

        let decoded = decode(XdrTypeName::StellarValue, &base64).expect("decodes");
        let re_encoded = encode(&decoded).expect("re-encodes");

        assert_eq!(base64, re_encoded);
        match decoded {
            DecodedValue::StellarValue(v) => assert_eq!(v, value),
            DecodedValue::ContractExecutable(_) => panic!("expected StellarValue"),
            DecodedValue::ScVal(_) => panic!("expected StellarValue"),
            DecodedValue::TransactionEnvelope(_) => panic!("expected StellarValue"),
            DecodedValue::LedgerEntry(_) => panic!("expected StellarValue"),
            DecodedValue::LedgerHeader(_) => panic!("expected StellarValue"),
            DecodedValue::LedgerCloseMeta(_) => panic!("expected StellarValue"),
        }
    }

    #[test]
    fn encoding_a_decoded_transaction_envelope_reproduces_the_same_bytes() {
        let original = TransactionEnvelope::default();
        let base64 = encode_transaction_envelope(&original).expect("encodes");

        let decoded = decode_transaction_envelope(&base64).expect("decodes");
        let re_encoded = encode_transaction_envelope(&decoded).expect("re-encodes");

        assert_eq!(base64, re_encoded);
    }

    /// The generic [`encode`] path must agree with the typed
    /// [`encode_transaction_envelope`] for the same envelope, so fixtures
    /// that go through `DecodedValue` are not encoded differently.
    #[test]
    fn the_generic_encode_path_agrees_with_the_typed_one_for_envelopes() {
        let envelope = TransactionEnvelope::default();
        let typed = encode_transaction_envelope(&envelope).expect("encodes");
        let generic =
            encode(&DecodedValue::TransactionEnvelope(Box::new(envelope))).expect("encodes");
        assert_eq!(typed, generic);
    }

    #[test]
    fn encoding_a_decoded_ledger_entry_reproduces_the_same_bytes() {
        let original = LedgerEntry::default();
        let base64 = encode_ledger_entry(&original).expect("encodes");

        let decoded = decode_ledger_entry(&base64).expect("decodes");
        let re_encoded = encode_ledger_entry(&decoded).expect("re-encodes");

        assert_eq!(base64, re_encoded);
        assert_eq!(decoded, original);
    }

    /// The generic [`encode`] path must agree with the typed
    /// [`encode_ledger_entry`] for the same entry, so fixtures that go
    /// through `DecodedValue` are not encoded differently.
    #[test]
    fn the_generic_encode_path_agrees_with_the_typed_one_for_ledger_entries() {
        let entry = LedgerEntry::default();
        let typed = encode_ledger_entry(&entry).expect("encodes");
        let generic = encode(&DecodedValue::LedgerEntry(Box::new(entry))).expect("encodes");
        assert_eq!(typed, generic);
    }

    #[test]
    fn encoding_a_decoded_ledger_header_reproduces_the_same_bytes() {
        let original = LedgerHeader::default();
        let base64 = encode_ledger_header(&original).expect("encodes");

        let decoded = decode_ledger_header(&base64).expect("decodes");
        let re_encoded = encode_ledger_header(&decoded).expect("re-encodes");

        assert_eq!(base64, re_encoded);
        assert_eq!(decoded, original);
    }

    /// The generic [`encode`] path must agree with the typed
    /// [`encode_ledger_header`] for the same header, so fixtures that go
    /// through `DecodedValue` are not encoded differently.
    #[test]
    fn the_generic_encode_path_agrees_with_the_typed_one_for_ledger_headers() {
        let header = LedgerHeader::default();
        let typed = encode_ledger_header(&header).expect("encodes");
        let generic = encode(&DecodedValue::LedgerHeader(Box::new(header))).expect("encodes");
        assert_eq!(typed, generic);
    }

    #[test]
    fn encoding_a_decoded_ledger_close_meta_reproduces_the_same_bytes() {
        let original = LedgerCloseMeta::default();
        let base64 = encode_ledger_close_meta(&original).expect("encodes");

        let decoded = decode_ledger_close_meta(&base64).expect("decodes");
        let re_encoded = encode_ledger_close_meta(&decoded).expect("re-encodes");

        assert_eq!(base64, re_encoded);
        assert_eq!(decoded, original);
    }

    /// The generic [`encode`] path must agree with the typed
    /// [`encode_ledger_close_meta`] for the same metadata, so fixtures that go
    /// through `DecodedValue` are not encoded differently.
    #[test]
    fn the_generic_encode_path_agrees_with_the_typed_one_for_ledger_close_meta() {
        let meta = LedgerCloseMeta::default();
        let typed = encode_ledger_close_meta(&meta).expect("encodes");
        let generic = encode(&DecodedValue::LedgerCloseMeta(Box::new(meta))).expect("encodes");
        assert_eq!(typed, generic);
    }

    /// Encodes one CAP-84 address-bearing `ScVal` through both the typed
    /// `stellar-xdr` path and the generic [`encode`] path, then decodes it
    /// through the fixture-facing [`decode`] entry point and asserts that all
    /// three agree.
    ///
    /// The expected base64 is reproduced in full rather than derived, so a
    /// change to the generated `ScAddress` layout fails here instead of
    /// silently redefining the expected bytes. The vectors are the canonical
    /// CAP-0084 structural vectors from the Protocol 30 fixture pack.
    fn assert_cap84_scval_roundtrip(value: ScVal, expected_base64: &str) {
        // The specialized typed encoder: serialising the concrete `ScVal`
        // type directly must produce the canonical protocol bytes.
        let specialized = value
            .to_xdr_base64(Limits::none())
            .expect("specialized encoding");
        assert_eq!(
            specialized, expected_base64,
            "the specialized encoder must reproduce the CAP-84 canonical vector"
        );

        // The generic encoder is the path fixtures take through
        // `DecodedValue`; it must agree byte for byte with the typed one.
        let generic = encode(&DecodedValue::ScVal(value.clone())).expect("generic encoding");
        assert_eq!(
            generic, specialized,
            "the generic encoder must agree with the specialized encoder"
        );

        // Decoding the generic bytes must recover the exact original value,
        // not merely an equal-looking address.
        let decoded = decode(XdrTypeName::ScVal, &generic).expect("decodes as ScVal");
        match decoded {
            DecodedValue::ScVal(ref decoded_value) => assert_eq!(*decoded_value, value),
            other => panic!("expected ScVal, got {other:?}"),
        }

        // And the decoded value must re-encode to the same bytes, proving the
        // round trip is symmetric through the generic path.
        let re_encoded = encode(&decoded).expect("re-encodes");
        assert_eq!(re_encoded, generic);
    }

    /// CAP-84's `SC_ADDRESS_TYPE_MUXED_ACCOUNT` arm: an 8-byte id followed by
    /// a 32-byte ed25519 key inside `ScVal::Address`.
    #[test]
    fn cap84_muxed_account_scval_roundtrips_through_the_generic_encoder() {
        let value = ScVal::Address(ScAddress::MuxedAccount(MuxedEd25519Account {
            id: 0x0102_0304_0506_0708,
            ed25519: Uint256([0x33; 32]),
        }));

        assert_cap84_scval_roundtrip(
            value,
            "AAAAEgAAAAIBAgMEBQYHCDMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMz",
        );
    }

    /// CAP-84's `SC_ADDRESS_TYPE_MUXED_CONTRACT` arm: an 8-byte id followed by
    /// a 32-byte contract hash inside `ScVal::Address`.
    #[test]
    fn cap84_muxed_contract_scval_roundtrips_through_the_generic_encoder() {
        let value = ScVal::Address(ScAddress::MuxedContract(MuxedContract {
            id: 0x0102_0304_0506_0708,
            contract_id: ContractId(Hash([0x22; 32])),
        }));

        assert_cap84_scval_roundtrip(
            value,
            "AAAAEgAAAAUBAgMEBQYHCCIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi",
        );
    }

    #[test]
    fn encoding_a_decoded_contract_executable_reproduces_the_same_value() {
        let value = ContractExecutable::Wasm(stellar_xdr::Hash([7; 32]));
        let base64 = encode(&DecodedValue::ContractExecutable(value.clone())).expect("encodes");

        let decoded = decode(XdrTypeName::ContractExecutable, &base64).expect("decodes");
        let re_encoded = encode(&decoded).expect("re-encodes");

        assert_eq!(base64, re_encoded);
        match decoded {
            DecodedValue::ContractExecutable(v) => assert_eq!(v, value),
            DecodedValue::StellarValue(_) => panic!("expected ContractExecutable"),
            DecodedValue::ScVal(_) => panic!("expected ContractExecutable"),
            DecodedValue::TransactionEnvelope(_) => panic!("expected ContractExecutable"),
            DecodedValue::LedgerEntry(_) => panic!("expected ContractExecutable"),
            DecodedValue::LedgerHeader(_) => panic!("expected ContractExecutable"),
            DecodedValue::LedgerCloseMeta(_) => panic!("expected ContractExecutable"),
        }
    }
}
