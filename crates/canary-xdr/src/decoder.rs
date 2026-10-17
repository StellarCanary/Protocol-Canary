//! Decoding XDR values by type name, using the official `stellar-xdr`
//! crate's generated types.
//!
//! The set of supported type names starts small (just what the Protocol 28
//! fixture pack needs, per the project's dependency-discipline rule) and
//! grows one match arm at a time as new fixtures require new types — it is
//! not meant to become a fully generic dispatcher.

use std::fmt;

use stellar_xdr::{
    ContractExecutable, LedgerEntry, Limits, ReadXdr, StellarValue, TransactionEnvelope,
};

/// A type name a fixture can name in its `type` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XdrTypeName {
    StellarValue,
    ContractExecutable,
    ScVal,
    TransactionEnvelope,
    LedgerEntry,
}

impl fmt::Display for XdrTypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XdrTypeName::StellarValue => f.write_str("StellarValue"),
            XdrTypeName::ContractExecutable => f.write_str("ContractExecutable"),
            XdrTypeName::ScVal => f.write_str("ScVal"),
            XdrTypeName::TransactionEnvelope => f.write_str("TransactionEnvelope"),
            XdrTypeName::LedgerEntry => f.write_str("LedgerEntry"),
        }
    }
}

impl std::str::FromStr for XdrTypeName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "StellarValue" => Ok(XdrTypeName::StellarValue),
            "ContractExecutable" => Ok(XdrTypeName::ContractExecutable),
            "ScVal" => Ok(XdrTypeName::ScVal),
            "TransactionEnvelope" => Ok(XdrTypeName::TransactionEnvelope),
            "LedgerEntry" => Ok(XdrTypeName::LedgerEntry),
            other => Err(format!(
                "unsupported XDR type name {other:?}: supported types are: StellarValue, \
                 ContractExecutable, ScVal, TransactionEnvelope, LedgerEntry"
            )),
        }
    }
}

/// A decoded XDR value, tagged by which concrete type it holds.
#[derive(Debug, Clone)]
pub enum DecodedValue {
    StellarValue(StellarValue),
    ContractExecutable(ContractExecutable),
    ScVal(stellar_xdr::ScVal),
    /// Boxed because a full `TransactionEnvelope` dwarfs the other
    /// variants; the indirection keeps `DecodedValue` small enough to move.
    TransactionEnvelope(Box<TransactionEnvelope>),
    /// Boxed because a full `LedgerEntry` (a `ContractCode` entry can carry
    /// an entire WASM module) dwarfs the other variants; the indirection
    /// keeps `DecodedValue` small enough to move.
    LedgerEntry(Box<LedgerEntry>),
}

impl DecodedValue {
    /// The [`XdrTypeName`] of the concrete variant this value holds.
    ///
    /// `DecodedValue` is a tagged enum: whichever variant was produced by
    /// [`decode`] (`StellarValue`, `ContractExecutable`, or `ScVal`), this
    /// reports the matching [`XdrTypeName`] without inspecting the payload.
    /// The input is simply the decoded value itself (`&self`).
    ///
    /// This is infallible: it cannot fail and never panics, because every
    /// variant maps to exactly one type name.
    ///
    /// ```
    /// use canary_xdr::{decode, XdrTypeName};
    ///
    /// // `AAAAAQ==` is the XDR encoding of `ScVal::Void`.
    /// let value = decode(XdrTypeName::ScVal, "AAAAAQ==").unwrap();
    /// assert_eq!(value.type_name(), XdrTypeName::ScVal);
    /// ```
    pub fn type_name(&self) -> XdrTypeName {
        match self {
            DecodedValue::StellarValue(_) => XdrTypeName::StellarValue,
            DecodedValue::ContractExecutable(_) => XdrTypeName::ContractExecutable,
            DecodedValue::ScVal(_) => XdrTypeName::ScVal,
            DecodedValue::TransactionEnvelope(_) => XdrTypeName::TransactionEnvelope,
            DecodedValue::LedgerEntry(_) => XdrTypeName::LedgerEntry,
        }
    }
}

/// Why a base64 XDR value could not be decoded, with the byte offset at
/// which decoding failed.
///
/// Returned by the typed decode entry points such as
/// [`decode_transaction_envelope`] or [`decode_ledger_entry`]. Unlike a
/// bare `stellar_xdr::Error`, this separates input that is not base64 at
/// all from input whose decoded
/// bytes are not valid XDR, and it names the offset *into the decoded XDR
/// bytes* at which the parse stopped — the information needed to debug a
/// truncated or corrupt capture.
#[derive(Debug, thiserror::Error)]
pub enum XdrDecodeError {
    /// The input is not decodable base64, so no XDR bytes exist to point
    /// into; the offset is therefore always [`XdrDecodeError::byte_offset`]
    /// `0`.
    #[error("input is not valid base64: {0}")]
    InvalidBase64(#[from] base64::DecodeError),

    /// The decoded bytes are not a valid value of `type_name`: the buffer
    /// was truncated, corrupt, or carries trailing bytes after a complete
    /// value.
    #[error("failed to decode {type_name} XDR at byte offset {offset}: {message}")]
    InvalidXdr {
        type_name: XdrTypeName,
        offset: u64,
        message: String,
    },
}

impl XdrDecodeError {
    /// The offset into the decoded XDR bytes at which decoding failed.
    ///
    /// Always `0` for [`XdrDecodeError::InvalidBase64`] (there are no decoded
    /// bytes to index into), and the parser's consumed-byte count for
    /// [`XdrDecodeError::InvalidXdr`].
    pub fn byte_offset(&self) -> u64 {
        match self {
            XdrDecodeError::InvalidBase64(_) => 0,
            XdrDecodeError::InvalidXdr { offset, .. } => *offset,
        }
    }
}

/// Decodes `base64_text` as `T`, reporting the byte offset at which a
/// failure occurred.
///
/// This mirrors what `stellar_xdr`'s `from_xdr_base64` does (base64-decode,
/// then read exactly one XDR value from the bytes and require that they are
/// all consumed) but reads through a [`std::io::Cursor`] it owns, so the
/// parser's position is still available when the read fails. Internal
/// whitespace is ignored, matching `from_xdr_base64`.
fn read_xdr_from_base64<T: ReadXdr>(
    type_name: XdrTypeName,
    base64_text: &str,
) -> Result<T, XdrDecodeError> {
    use base64::Engine as _;

    let cleaned: String = base64_text.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(XdrDecodeError::InvalidBase64)?;

    let mut cursor = std::io::Cursor::new(bytes.as_slice());
    let mut limited = stellar_xdr::Limited::new(&mut cursor, Limits::none());
    let value = T::read_xdr(&mut limited).map_err(|error| XdrDecodeError::InvalidXdr {
        type_name,
        offset: limited.inner.position(),
        message: error.to_string(),
    })?;
    let consumed = limited.inner.position();

    let total = bytes.len() as u64;
    if consumed != total {
        return Err(XdrDecodeError::InvalidXdr {
            type_name,
            offset: consumed,
            message: format!(
                "{} trailing byte(s) after a complete {type_name} value",
                total - consumed
            ),
        });
    }
    Ok(value)
}

/// Decodes base64 TransactionEnvelope XDR (V0, V1, or FeeBump).
///
/// Fails with [`XdrDecodeError`] when the input is not valid base64, when
/// the decoded bytes are not a complete `TransactionEnvelope`, or when
/// bytes remain after the value. Re-encoding the result with
/// [`crate::encode_transaction_envelope`] reproduces the input byte for
/// byte (see `tests/envelope_roundtrip.rs`).
pub fn decode_transaction_envelope(
    base64_text: &str,
) -> Result<TransactionEnvelope, XdrDecodeError> {
    read_xdr_from_base64(XdrTypeName::TransactionEnvelope, base64_text)
}

/// Decodes base64 LedgerEntry XDR.
///
/// Covers every `LedgerEntryData` variant the engine supports — Account,
/// Trustline, ContractData, ContractCode, and the rest — because
/// `LedgerEntry` is decoded as a whole, including the entry's
/// `lastModifiedLedgerSeq` and `ext` (so a `ContractCode` entry's V1
/// extension with its cost inputs survives a roundtrip untouched).
///
/// Fails with [`XdrDecodeError`] when the input is not valid base64, when
/// the decoded bytes are not a complete `LedgerEntry`, or when bytes remain
/// after the value. Re-encoding the result with
/// [`crate::encode_ledger_entry`] reproduces the input byte for byte (see
/// `tests/ledger_entry_roundtrip.rs`).
pub fn decode_ledger_entry(base64_text: &str) -> Result<LedgerEntry, XdrDecodeError> {
    read_xdr_from_base64(XdrTypeName::LedgerEntry, base64_text)
}

/// Decodes `base64` as the named XDR type.
///
/// An error here is a real decode failure, not a tool bug: it is the
/// caller's job to turn this into either a `Status::Fail` (when the
/// fixture expects success) or confirmation of an expected rejection
/// (`decode-failure` fixtures).
pub fn decode(type_name: XdrTypeName, base64: &str) -> Result<DecodedValue, stellar_xdr::Error> {
    match type_name {
        XdrTypeName::StellarValue => {
            StellarValue::from_xdr_base64(base64, Limits::none()).map(DecodedValue::StellarValue)
        }
        XdrTypeName::ContractExecutable => {
            ContractExecutable::from_xdr_base64(base64, Limits::none())
                .map(DecodedValue::ContractExecutable)
        }
        XdrTypeName::ScVal => {
            stellar_xdr::ScVal::from_xdr_base64(base64, Limits::none()).map(DecodedValue::ScVal)
        }
        XdrTypeName::TransactionEnvelope => {
            TransactionEnvelope::from_xdr_base64(base64, Limits::none())
                .map(|envelope| DecodedValue::TransactionEnvelope(Box::new(envelope)))
        }
        XdrTypeName::LedgerEntry => LedgerEntry::from_xdr_base64(base64, Limits::none())
            .map(|entry| DecodedValue::LedgerEntry(Box::new(entry))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_type_names() {
        assert_eq!(
            "StellarValue".parse::<XdrTypeName>().unwrap(),
            XdrTypeName::StellarValue
        );
    }

    #[test]
    fn rejects_unknown_type_names() {
        assert!("NotARealType".parse::<XdrTypeName>().is_err());
    }

    #[test]
    fn decode_fails_on_garbage_input() {
        let result = decode(XdrTypeName::StellarValue, "not-valid-base64-xdr!!!");
        assert!(result.is_err());
    }

    #[test]
    fn parses_the_transaction_envelope_type_name() {
        assert_eq!(
            "TransactionEnvelope".parse::<XdrTypeName>().unwrap(),
            XdrTypeName::TransactionEnvelope
        );
        assert_eq!(
            XdrTypeName::TransactionEnvelope.to_string(),
            "TransactionEnvelope"
        );
    }

    #[test]
    fn parses_the_ledger_entry_type_name() {
        assert_eq!(
            "LedgerEntry".parse::<XdrTypeName>().unwrap(),
            XdrTypeName::LedgerEntry
        );
        assert_eq!(XdrTypeName::LedgerEntry.to_string(), "LedgerEntry");
    }

    #[test]
    fn decode_transaction_envelope_decodes_a_valid_envelope() {
        use stellar_xdr::WriteXdr;

        let base64 = TransactionEnvelope::default()
            .to_xdr_base64(Limits::none())
            .unwrap();
        let decoded = decode_transaction_envelope(&base64).unwrap();
        assert_eq!(decoded, TransactionEnvelope::default());
    }

    #[test]
    fn decode_ledger_entry_decodes_a_valid_entry() {
        use stellar_xdr::WriteXdr;

        let base64 = LedgerEntry::default()
            .to_xdr_base64(Limits::none())
            .unwrap();
        let decoded = decode_ledger_entry(&base64).unwrap();
        assert_eq!(decoded, LedgerEntry::default());
    }

    /// Input that is not base64 at all has no decoded bytes to point into,
    /// so it is a distinct variant with offset 0 — never a parse error at
    /// some fabricated position.
    #[test]
    fn corrupted_base64_is_reported_as_invalid_base64_at_offset_zero() {
        let err = decode_transaction_envelope("not valid base64!!!").unwrap_err();
        assert!(matches!(err, XdrDecodeError::InvalidBase64(_)), "{err:?}");
        assert_eq!(err.byte_offset(), 0);

        let err = decode_ledger_entry("not valid base64!!!").unwrap_err();
        assert!(matches!(err, XdrDecodeError::InvalidBase64(_)), "{err:?}");
        assert_eq!(err.byte_offset(), 0);
    }

    /// A buffer that ends mid-value must fail with a byte offset inside the
    /// buffer, so a truncated capture can be diagnosed from the error alone.
    #[test]
    fn truncated_xdr_is_reported_with_a_byte_offset() {
        use base64::Engine as _;
        use stellar_xdr::WriteXdr;

        let base64 = TransactionEnvelope::default()
            .to_xdr_base64(Limits::none())
            .unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&base64)
            .unwrap();
        let truncated = &bytes[..bytes.len() - 8];
        let truncated_base64 = base64::engine::general_purpose::STANDARD.encode(truncated);

        let err = decode_transaction_envelope(&truncated_base64).unwrap_err();
        match &err {
            XdrDecodeError::InvalidXdr {
                type_name, offset, ..
            } => {
                assert_eq!(*type_name, XdrTypeName::TransactionEnvelope);
                assert!(
                    *offset > 0 && *offset <= truncated.len() as u64,
                    "offset must point into the truncated buffer, got {offset}"
                );
            }
            other => panic!("expected InvalidXdr, got {other:?}"),
        }
    }

    /// A complete value followed by extra bytes is rejected (the same
    /// all-bytes-consumed rule as `from_xdr_base64`), with the offset at
    /// the end of the value where the trailing bytes start.
    #[test]
    fn trailing_bytes_after_a_complete_value_are_rejected() {
        use base64::Engine as _;
        use stellar_xdr::WriteXdr;

        let base64 = TransactionEnvelope::default()
            .to_xdr_base64(Limits::none())
            .unwrap();
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(&base64)
            .unwrap();
        let value_len = bytes.len() as u64;
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        let with_trailing = base64::engine::general_purpose::STANDARD.encode(bytes);

        let err = decode_transaction_envelope(&with_trailing).unwrap_err();
        match &err {
            XdrDecodeError::InvalidXdr {
                offset, message, ..
            } => {
                assert_eq!(*offset, value_len, "offset must be the end of the value");
                assert!(message.contains("trailing"), "message: {message}");
            }
            other => panic!("expected InvalidXdr, got {other:?}"),
        }
    }
}
