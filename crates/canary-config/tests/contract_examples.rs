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

/// Every key and table header the canonical form can contain, in the order
/// CF-03 section 3 writes them.
const CANONICAL_ORDER: [&str; 9] = [
    "lockVersion",
    "[tool]",
    "version",
    "[fixtures]",
    "protocol",
    "packDigest",
    "registryVersion",
    "source",
    "revision",
];

/// The key or table header each non-blank line of `text` starts with.
fn entries(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match line.split_once(" = ") {
            Some((key, _)) => key,
            None => line,
        })
        .collect()
}

#[test]
fn the_canonical_form_orders_keys_as_the_contract_does() {
    let blocks = marked_blocks("<!-- contract-example: lock-v1 -->");
    let lock = parse_lock(&blocks[0], Path::new(LOCK_FILE_NAME)).unwrap();
    let canonical = serialize_lock(&lock).unwrap();

    let positions: Vec<usize> = entries(&canonical)
        .into_iter()
        .map(|entry| {
            CANONICAL_ORDER
                .iter()
                .position(|known| *known == entry)
                .unwrap_or_else(|| panic!("unexpected entry {entry:?} in:\n{canonical}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "keys are out of contract order:\n{canonical}"
    );
}

#[test]
fn the_order_of_keys_in_the_input_does_not_change_the_canonical_bytes() {
    let blocks = marked_blocks("<!-- contract-example: lock-v1 -->");
    let lock = parse_lock(&blocks[0], Path::new(LOCK_FILE_NAME)).unwrap();
    let canonical = serialize_lock(&lock).unwrap();

    // Write the same data with the [fixtures] keys in reverse order.
    let (head, fixtures) = canonical
        .split_once("[fixtures]\n")
        .expect("the example has a [fixtures] table");
    let reversed: Vec<&str> = fixtures.lines().rev().collect();
    let shuffled = format!("{head}[fixtures]\n{}\n", reversed.join("\n"));
    assert_ne!(shuffled, canonical, "the input must really be reordered");

    let reparsed = parse_lock(&shuffled, Path::new(LOCK_FILE_NAME)).unwrap();
    assert_eq!(reparsed, lock);
    assert_eq!(serialize_lock(&reparsed).unwrap(), canonical);
}

#[test]
fn repeated_roundtrips_of_the_example_never_drift() {
    let blocks = marked_blocks("<!-- contract-example: lock-v1 -->");
    let lock = parse_lock(&blocks[0], Path::new(LOCK_FILE_NAME)).unwrap();

    let first = serialize_lock(&lock).unwrap();
    let mut text = first.clone();
    for round in 0..5 {
        let parsed = parse_lock(&text, Path::new(LOCK_FILE_NAME)).unwrap();
        text = serialize_lock(&parsed).unwrap();
        assert_eq!(text.as_bytes(), first.as_bytes(), "drift in round {round}");
    }
}

#[test]
fn the_canonical_form_drops_the_examples_comments_and_keeps_one_lf_ending() {
    let blocks = marked_blocks("<!-- contract-example: lock-v1 -->");
    let source = &blocks[0];
    let lock = parse_lock(source, Path::new(LOCK_FILE_NAME)).unwrap();
    let canonical = serialize_lock(&lock).unwrap();

    // The documented example carries an illustrative comment; the canonical
    // form must not, and so differs from the source text.
    assert!(
        source.lines().any(|line| line.contains('#')),
        "the CF-03 example is expected to carry a comment"
    );
    assert!(canonical.lines().all(|line| !line.contains('#')));
    assert_ne!(&canonical, source);

    assert!(!canonical.contains('\r'));
    assert!(canonical.ends_with('\n') && !canonical.ends_with("\n\n"));
    assert!(canonical.lines().all(|line| line == line.trim_end()));
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
