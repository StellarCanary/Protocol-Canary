//! Keeps the JSON examples in `docs/contracts/cf-01-report.md` honest.
//!
//! A fenced `json` block preceded by `<!-- contract-example: report-v1-valid -->`
//! must be accepted by `JsonReporter::parse`; one preceded by
//! `<!-- contract-example: report-v1-invalid -->` must be rejected.

use canary_report::JsonReporter;

const DOC: &str = include_str!("../../../docs/contracts/cf-01-report.md");

/// Returns the bodies of all fenced `json` blocks that follow `marker`.
fn marked_blocks(marker: &str) -> Vec<&'static str> {
    let mut blocks = Vec::new();
    let mut rest = DOC;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let fence = "```json\n";
        let start = rest
            .find(fence)
            .expect("marker is followed by a json block")
            + fence.len();
        let end = rest[start..].find("```").expect("json block is closed") + start;
        blocks.push(&rest[start..end]);
        rest = &rest[end..];
    }
    blocks
}

#[test]
fn valid_examples_parse() {
    let blocks = marked_blocks("<!-- contract-example: report-v1-valid -->");
    assert_eq!(blocks.len(), 2, "expected two valid examples in CF-01");
    for block in blocks {
        JsonReporter::parse(block).unwrap_or_else(|e| panic!("example rejected: {e}\n{block}"));
    }
}

#[test]
fn invalid_examples_are_rejected() {
    let blocks = marked_blocks("<!-- contract-example: report-v1-invalid -->");
    assert_eq!(blocks.len(), 1, "expected one invalid example in CF-01");
    for block in blocks {
        assert!(
            JsonReporter::parse(block).is_err(),
            "invalid example was accepted:\n{block}"
        );
    }
}
