//! Keeps the TOML fixture examples in `docs/fixtures/scval.md` honest.
//!
//! A fenced `toml` block preceded by `<!-- contract-example: scval-valid -->`
//! must be accepted by `canary_fixtures::parse_fixture_str` and the relevant
//! surface runner (`canary_soroban::SorobanFixture` or `canary_xdr::XdrFixture`).
//!
//! A fenced `toml` block preceded by `<!-- contract-example: scval-invalid -->`
//! must be rejected during metadata or surface validation.
//!
//! Malformed base64 payloads derived from those examples (issue #746) must
//! be rejected with an `Err` that names the offending field and reason —
//! never a panic — while the valid examples keep decoding to the exact XDR
//! values the documentation promises.

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use canary_core::Surface;
use canary_fixtures::parse_fixture_str;
use canary_soroban::{ScValInput, SorobanFixture};
use canary_xdr::{XdrFixture, XdrTypeName};
use serde_json::{json, Value};

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

// ---------------------------------------------------------------------------
// Malformed base64 payloads (issue #746)
// ---------------------------------------------------------------------------

/// The valid example whose body contains `needle`, so the mutations below
/// stay tied to what the documentation actually shows.
fn valid_example_containing(needle: &str) -> String {
    marked_blocks("<!-- contract-example: scval-valid -->")
        .into_iter()
        .find(|block| block.contains(needle))
        .unwrap_or_else(|| panic!("scval.md has no valid example containing {needle:?}"))
}

/// Runs `f`, failing the test with `what` if it panics instead of
/// returning.
fn without_panic<T>(what: &str, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f))
        .unwrap_or_else(|_| panic!("{what} panicked instead of returning an error"))
}

/// The documented base64 `bytes` argument (`3q2+7w==` is `de ad be ef`).
const DOC_BASE64_BYTES: &str = "\"3q2+7w==\"";

/// Malformed replacements for [`DOC_BASE64_BYTES`], each with the phrase
/// the Soroban builder's error must contain.
const MALFORMED_BYTES: &[(&str, &str)] = &[
    ("\"3q2+7w=\"", "multiple of 4"),
    ("\"3q2+7w\"", "multiple of 4"),
    ("\"3q2*7w==\"", "is not a base64 character"),
    ("\"3q2-7w==\"", "is not a base64 character"),
    ("\"3q2+7w=A\"", "is not a base64 character"),
    ("\"3q2+7x==\"", "non-zero trailing bits"),
    // Padding in the middle of the payload.
    ("\"3q2+7w==3q2+7w==\"", "is not a base64 character"),
    ("\"3===\"", "too much padding"),
];

#[test]
fn malformed_base64_bytes_arguments_fail_with_descriptive_errors() {
    let example = valid_example_containing(DOC_BASE64_BYTES);

    for (index, (replacement, reason)) in MALFORMED_BYTES.iter().enumerate() {
        let block = example.replace(DOC_BASE64_BYTES, replacement);
        assert_ne!(block, example, "mutation #{index} did not apply");
        let path = Path::new("tests/fixtures/doc_example_bad_bytes.toml");

        let error = without_panic(&format!("bytes mutation {replacement}"), || {
            let loaded = parse_fixture_str(&block, path).map_err(|e| e.to_string())?;
            let fixture = SorobanFixture::from_loaded(&loaded).map_err(|e| e.to_string())?;
            canary_soroban::build_invoke_transaction_envelope(&fixture.invocation)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .expect_err(&format!(
            "malformed base64 bytes {replacement} must be rejected"
        ));

        assert!(
            error.contains("bytes"),
            "error for {replacement} must name the bytes argument: {error}"
        );
        assert!(
            error.contains(reason),
            "error for {replacement} must explain {reason:?}: {error}"
        );
    }
}

#[test]
fn malformed_base64_xdr_values_are_rejected_when_the_fixture_is_loaded() {
    let example = valid_example_containing("kind = \"roundtrip\"");
    let documented = "\"AAAAAwAAACo=\"";
    assert!(example.contains(documented), "{example}");

    let malformed: &[(&str, &str)] = &[
        // Missing padding.
        ("\"AAAAAwAAACo\"", "base64"),
        // A character outside the standard alphabet.
        ("\"AAAA*wAAACo=\"", "not valid base64"),
        // URL-safe alphabet instead of the standard one.
        ("\"AAAA_wAAACo=\"", "not valid base64"),
        // Non-zero trailing bits: decodes leniently but is not canonical.
        ("\"AAAAAwAAACp=\"", "base64"),
        // Truncated mid-quantum.
        ("\"AAAAAwA\"", "base64"),
    ];

    for (replacement, reason) in malformed {
        let block = example.replace(documented, replacement);
        let path = Path::new("tests/fixtures/doc_example_bad_xdr.toml");

        let error = without_panic(&format!("xdr mutation {replacement}"), || {
            let loaded = parse_fixture_str(&block, path).map_err(|e| e.to_string())?;
            XdrFixture::from_loaded(&loaded)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .expect_err(&format!(
            "malformed base64 XDR {replacement} must be rejected"
        ));

        assert!(
            error.contains("value_base64"),
            "error for {replacement} must name the value_base64 field: {error}"
        );
        assert!(
            error.contains(reason),
            "error for {replacement} must explain {reason:?}: {error}"
        );
        assert!(
            error.contains("doc_example_bad_xdr.toml"),
            "error for {replacement} must point at the fixture file: {error}"
        );
    }
}

#[test]
fn the_xdr_decoder_returns_errors_for_malformed_base64_scvals() {
    for malformed in [
        "AAAAAwAAACo",
        "AAAA*wAAACo=",
        "====",
        "A",
        "AAAAAwAAACo=AAAA",
    ] {
        let result = without_panic(&format!("decoding {malformed:?}"), || {
            canary_xdr::decode(XdrTypeName::ScVal, malformed)
        });
        let error = result.expect_err(&format!("{malformed:?} must not decode as ScVal"));
        assert!(
            !error.to_string().is_empty(),
            "decode error for {malformed:?} must carry a message"
        );
    }

    // The documented decode-failure payload is well-formed base64 whose XDR
    // is invalid: still an `Err`, never a panic.
    let example = valid_example_containing("kind = \"decode-failure\"");
    assert!(example.contains("\"////AAAAAAA=\""), "{example}");
    let result = without_panic("decoding the documented decode-failure payload", || {
        canary_xdr::decode(XdrTypeName::ScVal, "////AAAAAAA=")
    });
    assert!(
        result.is_err(),
        "an invalid ScVal discriminant must not decode"
    );
}

/// Every `{"bytes": "<hex>"}` object anywhere in `value`, in document order.
fn bytes_values(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            if fields.len() == 1 {
                if let Some(Value::String(hex)) = fields.get("bytes") {
                    out.push(hex.clone());
                }
            }
            fields.values().for_each(|field| bytes_values(field, out));
        }
        Value::Array(items) => items.iter().for_each(|item| bytes_values(item, out)),
        _ => {}
    }
}

#[test]
fn valid_base64_doc_examples_still_decode_to_the_expected_xdr() {
    // XDR surface: the documented roundtrip payload is ScVal::U32(42).
    let xdr_example = valid_example_containing("kind = \"roundtrip\"");
    let loaded = parse_fixture_str(&xdr_example, Path::new("tests/fixtures/doc_xdr.toml"))
        .expect("documented XDR example parses");
    XdrFixture::from_loaded(&loaded).expect("documented XDR example is a valid fixture");
    let decoded =
        canary_xdr::decode(XdrTypeName::ScVal, "AAAAAwAAACo=").expect("documented ScVal decodes");
    assert_eq!(decoded.type_name(), XdrTypeName::ScVal);
    assert_eq!(decoded.to_json().unwrap(), json!({ "u32": 42 }));

    // Soroban surface: the hex and base64 `bytes` arguments land in the
    // built envelope as the exact bytes the documentation describes.
    let soroban_example = valid_example_containing(DOC_BASE64_BYTES);
    let loaded = parse_fixture_str(
        &soroban_example,
        Path::new("tests/fixtures/doc_soroban.toml"),
    )
    .expect("documented Soroban example parses");
    let fixture = SorobanFixture::from_loaded(&loaded).expect("documented Soroban body parses");
    assert!(fixture
        .invocation
        .args
        .contains(&ScValInput::Bytes("3q2+7w==".to_string())));

    let envelope = canary_soroban::build_invoke_transaction_envelope(&fixture.invocation)
        .expect("documented Soroban example builds an envelope");
    let envelope = canary_xdr::decode(XdrTypeName::TransactionEnvelope, &envelope)
        .expect("built envelope decodes")
        .to_json()
        .expect("envelope renders as JSON");

    let mut bytes = Vec::new();
    bytes_values(&envelope, &mut bytes);
    assert_eq!(
        bytes,
        vec!["deadbeefcafe".to_string(), "deadbeef".to_string()],
        "hex `0xdeadbeefcafe` and base64 `3q2+7w==` must encode as their raw bytes"
    );
}
