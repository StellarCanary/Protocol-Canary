//! Keeps the TOML examples in `docs/contracts/cf-03-lockfile.md` honest.
//!
//! A fenced `toml` block preceded by `<!-- contract-example: lock-v1 -->`
//! must be accepted by the lockfile parser; one preceded by
//! `<!-- contract-example: lock-v1-invalid -->` must be rejected.

use std::path::Path;

use canary_config::{parse_lock, serialize_lock, LOCK_FILE_NAME};

const DOC: &str = include_str!("../../../docs/contracts/cf-03-lockfile.md");

/// Returns the bodies of all fenced `toml` blocks that follow `marker`.
///
/// The document is normalized to `\n` first: a Windows checkout with
/// `core.autocrlf` stores it with `\r\n`, which would hide every fence.
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
fn the_valid_example_parses_and_reserializes_canonically() {
    let blocks = marked_blocks("<!-- contract-example: lock-v1 -->");
    assert_eq!(blocks.len(), 1, "expected one valid example in CF-03");

    let lock = parse_lock(&blocks[0], Path::new(LOCK_FILE_NAME))
        .unwrap_or_else(|e| panic!("example rejected: {e}\n{}", blocks[0]));

    // The example carries an illustrative comment, which the canonical form
    // never has, so compare the data rather than the bytes.
    let canonical = serialize_lock(&lock).unwrap();
    assert!(!canonical.contains('#'));
    let reparsed = parse_lock(&canonical, Path::new(LOCK_FILE_NAME)).unwrap();
    assert_eq!(reparsed, lock);
    assert_eq!(serialize_lock(&reparsed).unwrap(), canonical);
}

#[test]
fn the_invalid_examples_are_rejected() {
    let blocks = marked_blocks("<!-- contract-example: lock-v1-invalid -->");
    assert_eq!(blocks.len(), 2, "expected two invalid examples in CF-03");
    for block in blocks {
        assert!(
            parse_lock(&block, Path::new(LOCK_FILE_NAME)).is_err(),
            "invalid example was accepted:\n{block}"
        );
    }
}
