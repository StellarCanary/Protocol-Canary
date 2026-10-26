//! XDR compatibility runner for Stellar Protocol Canary.
//!
//! Decoding and encoding are delegated entirely to the official
//! `stellar-xdr` crate; this crate only adapts fixtures to it and turns the
//! outcome into a [`canary_core::CompatibilityResult`].

pub mod decoder;
pub mod encoder;
pub mod json;
pub mod runner;

pub use decoder::{
    decode, decode_ledger_entry, decode_transaction_envelope, DecodedValue, XdrDecodeError,
    XdrTypeName,
};
pub use encoder::{encode, encode_ledger_entry, encode_transaction_envelope};
pub use json::{compare_json, format_differences, JsonDifference, JsonDifferenceKind};
pub use runner::{DefaultXdrRunner, XdrAssertion, XdrError, XdrFixture, XdrRunner};
