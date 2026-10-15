//! CA-04 conformance vector: a Soroban contract project with a built
//! WASM artifact in the known Cargo output directory.
pub fn add(_env: (), a: i128, b: i128) -> i128 {
    a + b
}
