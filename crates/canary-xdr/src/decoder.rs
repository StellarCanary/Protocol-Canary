//! Decoding XDR values by type name, using the official `stellar-xdr`
//! crate's generated types.
//!
//! The set of supported type names starts small (just what the Protocol 28
//! fixture pack needs, per the project's dependency-discipline rule) and
//! grows one match arm at a time as new fixtures require new types — it is
//! not meant to become a fully generic dispatcher.

use std::fmt;

use stellar_xdr::{Limits, ReadXdr, StellarValue};

/// A type name a fixture can name in its `type` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XdrTypeName {
    StellarValue,
}

impl fmt::Display for XdrTypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XdrTypeName::StellarValue => f.write_str("StellarValue"),
        }
    }
}

impl std::str::FromStr for XdrTypeName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "StellarValue" => Ok(XdrTypeName::StellarValue),
            other => Err(format!(
                "unsupported XDR type name {other:?}: supported types are: StellarValue"
            )),
        }
    }
}

/// A decoded XDR value, tagged by which concrete type it holds.
#[derive(Debug, Clone)]
pub enum DecodedValue {
    StellarValue(StellarValue),
}

impl DecodedValue {
    pub fn type_name(&self) -> XdrTypeName {
        match self {
            DecodedValue::StellarValue(_) => XdrTypeName::StellarValue,
        }
    }
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
}
