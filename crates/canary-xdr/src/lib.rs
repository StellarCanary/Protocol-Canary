//! XDR compatibility runner for Stellar Protocol Canary.
//!
//! Decoding and encoding are delegated entirely to the official
//! `stellar-xdr` crate; this crate only adapts fixtures to it and turns the
//! outcome into a [`canary_core::CompatibilityResult`].

pub mod decoder;
pub mod encoder;
pub mod runner;

pub use decoder::{decode, decode_ledger_entry, DecodedValue, XdrDecodeError, XdrTypeName};
pub use encoder::{encode, encode_ledger_entry};
pub use runner::{DefaultXdrRunner, XdrAssertion, XdrError, XdrFixture, XdrRunner};
