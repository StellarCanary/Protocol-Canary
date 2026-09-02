//! Soroban simulation compatibility runner for Stellar Protocol Canary.
//!
//! No secret key is ever required and no transaction is ever submitted:
//! this crate only builds unsigned envelopes and simulates them.

pub mod builder;
pub mod runner;
pub mod simulation;

pub use builder::{build_invoke_transaction_envelope, BuilderError, InvocationSpec, ScValInput};
pub use runner::{
    DefaultSorobanRunner, SorobanAssertion, SorobanFixture, SorobanFixtureError, SorobanRunner,
};
pub use simulation::simulate;
