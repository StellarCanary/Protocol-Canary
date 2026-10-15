//! Re-encoding a previously decoded XDR value.

use stellar_xdr::{LedgerEntry, Limits, WriteXdr};

use crate::decoder::DecodedValue;

/// Re-encodes `value` back to base64 XDR.
pub fn encode(value: &DecodedValue) -> Result<String, stellar_xdr::Error> {
    match value {
        DecodedValue::StellarValue(v) => v.to_xdr_base64(Limits::none()),
        DecodedValue::ContractExecutable(v) => v.to_xdr_base64(Limits::none()),
        DecodedValue::ScVal(v) => v.to_xdr_base64(Limits::none()),
        DecodedValue::LedgerEntry(v) => v.to_xdr_base64(Limits::none()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{decode, decode_ledger_entry, DecodedValue, XdrTypeName};
    use stellar_xdr::{ContractExecutable, StellarValue};

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
            DecodedValue::LedgerEntry(_) => panic!("expected StellarValue"),
        }
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
            DecodedValue::LedgerEntry(_) => panic!("expected ContractExecutable"),
        }
    }
}
