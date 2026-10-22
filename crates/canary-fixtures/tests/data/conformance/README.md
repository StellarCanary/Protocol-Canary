# Conformance vectors

Vendored from `StellarCanary/ProtocolCanary-Fixtures` at commit
`ff61db29547a079adf718988a9636a9d691f5cfa`, directory
`tests/vectors/conformance/`.

The Rust harness in `../../conformance.rs` reads `vectors.json` and loads
each referenced fixture through `canary_fixtures::load_directory`, then
through the matching surface parser. Do not edit these files here; update
them from the fixtures repository and copy the corpus again.
