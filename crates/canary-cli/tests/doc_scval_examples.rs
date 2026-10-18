//! Keeps the TOML fixture examples in `docs/fixtures/scval.md` honest.
//!
//! A fenced `toml` block preceded by `<!-- contract-example: scval-valid -->`
//! must be accepted by `canary_fixtures::parse_fixture_str` and the relevant
//! surface runner (`canary_soroban::SorobanFixture` or `canary_xdr::XdrFixture`).
//!
//! A fenced `toml` block preceded by `<!-- contract-example: scval-invalid -->`
//! must be rejected during metadata or surface validation.

use std::collections::HashSet;
use std::path::Path;

use canary_core::Surface;
use canary_fixtures::parse_fixture_str;
use canary_soroban::{ScValInput, SorobanFixture};
use canary_xdr::XdrFixture;

const DOC: &str = include_str!("../../../docs/fixtures/scval.md");

/// Returns the bodies of all fenced `toml` blocks that follow `marker`.
fn marked_blocks(marker: &str) -> Vec<String> {
    let doc = DOC.replace("\r\n", "\n");
    let mut blocks = Vec::new();
    let mut rest = doc.as_str();
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let fence = "```toml\n";
        let start = rest
            .find(fence)
            .expect("marker is followed by a toml block")
            + fence.len();
        let end = rest[start..].find("```").expect("toml block is closed") + start;
        blocks.push(rest[start..end].to_string());
        rest = &rest[end..];
    }
    blocks
}

#[test]
fn all_valid_scval_fixture_examples_parse_cleanly() {
    let blocks = marked_blocks("<!-- contract-example: scval-valid -->");
    assert!(
        blocks.len() >= 5,
        "expected at least 5 valid fixture examples in scval.md, found {}",
        blocks.len()
    );

    let mut observed_kinds = HashSet::new();

    for (index, block) in blocks.iter().enumerate() {
        let fake_path = format!("tests/fixtures/doc_example_{index}.toml");
        let path = Path::new(&fake_path);

        let loaded = parse_fixture_str(block, path).unwrap_or_else(|e| {
            panic!("valid example #{index} failed to parse metadata: {e}\n{block}")
        });

        match loaded.metadata.surface {
            Surface::Soroban => {
                let soroban = SorobanFixture::from_loaded(&loaded).unwrap_or_else(|e| {
                    panic!("valid Soroban example #{index} failed to parse body: {e}\n{block}")
                });

                canary_soroban::build_invoke_transaction_envelope(&soroban.invocation)
                    .unwrap_or_else(|e| panic!("valid Soroban example #{index} failed to build transaction envelope: {e}\n{block}"));

                fn collect_kinds(args: &[ScValInput], set: &mut HashSet<&'static str>) {
                    for arg in args {
                        match arg {
                            ScValInput::Void => {
                                set.insert("void");
                            }
                            ScValInput::Bool(_) => {
                                set.insert("bool");
                            }
                            ScValInput::U32(_) => {
                                set.insert("u32");
                            }
                            ScValInput::I32(_) => {
                                set.insert("i32");
                            }
                            ScValInput::U64(_) => {
                                set.insert("u64");
                            }
                            ScValInput::I64(_) => {
                                set.insert("i64");
                            }
                            ScValInput::U128(_) => {
                                set.insert("u128");
                            }
                            ScValInput::I128(_) => {
                                set.insert("i128");
                            }
                            ScValInput::U256(_) => {
                                set.insert("u256");
                            }
                            ScValInput::I256(_) => {
                                set.insert("i256");
                            }
                            ScValInput::Duration(_) => {
                                set.insert("duration");
                            }
                            ScValInput::Timepoint(_) => {
                                set.insert("timepoint");
                            }
                            ScValInput::Symbol(_) => {
                                set.insert("symbol");
                            }
                            ScValInput::String(_) => {
                                set.insert("string");
                            }
                            ScValInput::Bytes(_) => {
                                set.insert("bytes");
                            }
                            ScValInput::Address(_) => {
                                set.insert("address");
                            }
                            ScValInput::Vec(items) => {
                                set.insert("vec");
                                collect_kinds(items, set);
                            }
                            ScValInput::Map(entries) => {
                                set.insert("map");
                                for (k, v) in entries {
                                    collect_kinds(&[k.clone(), v.clone()], set);
                                }
                            }
                        }
                    }
                }

                collect_kinds(&soroban.invocation.args, &mut observed_kinds);
            }
            Surface::Xdr => {
                XdrFixture::from_loaded(&loaded).unwrap_or_else(|e| {
                    panic!("valid XDR example #{index} failed to parse body: {e}\n{block}")
                });
            }
            Surface::Rpc => panic!("scval.md should only showcase soroban and xdr surfaces"),
        }
    }

    // Verify all 18 ScValInput variants are represented in the valid examples
    let expected_kinds = [
        "void",
        "bool",
        "u32",
        "i32",
        "u64",
        "i64",
        "u128",
        "i128",
        "u256",
        "i256",
        "duration",
        "timepoint",
        "symbol",
        "string",
        "bytes",
        "address",
        "vec",
        "map",
    ];

    for expected in expected_kinds {
        assert!(
            observed_kinds.contains(expected),
            "kind {expected:?} was not exercised in any valid documentation example"
        );
    }
}

#[test]
fn all_invalid_scval_fixture_examples_are_rejected() {
    let blocks = marked_blocks("<!-- contract-example: scval-invalid -->");
    assert!(
        blocks.len() >= 3,
        "expected at least 3 invalid fixture examples in scval.md, found {}",
        blocks.len()
    );

    for (index, block) in blocks.iter().enumerate() {
        let fake_path = format!("tests/fixtures/doc_example_invalid_{index}.toml");
        let path = Path::new(&fake_path);

        match parse_fixture_str(block, path) {
            Ok(loaded) => {
                // Metadata parsed; surface validation or envelope construction must reject it
                match loaded.metadata.surface {
                    Surface::Soroban => {
                        let invalid = match SorobanFixture::from_loaded(&loaded) {
                            Ok(fixture) => canary_soroban::build_invoke_transaction_envelope(
                                &fixture.invocation,
                            )
                            .is_err(),
                            Err(_) => true,
                        };
                        assert!(
                            invalid,
                            "invalid Soroban example #{index} unexpectedly succeeded:\n{block}"
                        );
                    }
                    Surface::Xdr => {
                        let result = XdrFixture::from_loaded(&loaded);
                        assert!(
                            result.is_err(),
                            "invalid XDR example #{index} unexpectedly succeeded:\n{block}"
                        );
                    }
                    Surface::Rpc => panic!("unexpected surface in scval.md invalid example"),
                }
            }
            Err(_) => {
                // Metadata parsing rejected it as invalid, which is also correct
            }
        }
    }
}
