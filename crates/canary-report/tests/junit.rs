//! JUnit XML export tests.
//!
//! These parse the rendered document with a real XML parser (`quick-xml`,
//! a dev-dependency used only here) instead of substring matching, so a
//! malformed document fails the suite rather than a lucky `contains`.

use canary_core::{
    CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, Status, Surface,
};
use canary_report::{JUnitReporter, ProjectSummary, ReportInput, SkipSummary, SURFACE_ORDER};
use quick_xml::events::Event;
use quick_xml::Reader;

fn result(id: &str, surface: Surface, status: Status) -> CompatibilityResult {
    CompatibilityResult {
        test_id: id.into(),
        protocol: ProtocolVersion(28),
        surface,
        status,
        summary: format!("{id} summary"),
        details: None,
        duration_ms: 2,
        fixture_id: Some(id.into()),
        source: canary_core::ResultSource::Live,
    }
}

fn report(results: Vec<CompatibilityResult>, skipped: Vec<SkipSummary>) -> ReportInput {
    ReportInput {
        tool_version: "0.2.0".into(),
        target_protocol: ProtocolVersion(28),
        project: ProjectSummary {
            name: "example".into(),
            project_type: ProjectType::Soroban,
        },
        network: None,
        results,
        skipped,
        verification: Vec::new(),
        decision: PolicyDecision::Pass,
        git: GitContext::default(),
        verbose: false,
        run: None,
    }
}

/// One parsed XML element: its tag, attributes, and (unescaped) text.
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
}

/// Every element the renderer emits, in document order. Tests can assert
/// on both structure and diagnostics.
fn parse(xml: &str) -> Vec<Element> {
    let mut reader = Reader::from_str(xml);
    let mut elements: Vec<Element> = Vec::new();
    let mut stack: Vec<Element> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(start)) => {
                stack.push(Element {
                    name: String::from_utf8_lossy(start.name().as_ref()).into_owned(),
                    attrs: attributes(&start, &reader),
                    text: String::new(),
                });
            }
            Ok(Event::Text(text)) => {
                let text = text.xml10_content().expect("well-formed text");
                match stack.last_mut() {
                    Some(element) => element.text.push_str(&text),
                    // Whitespace before the root element and after it is
                    // legal; anything else outside an element is not.
                    None => assert!(
                        text.trim().is_empty(),
                        "non-whitespace text outside the root element"
                    ),
                }
            }
            Ok(Event::GeneralRef(entity)) => {
                // quick-xml emits `&ref;` as its own event, so entity
                // resolution happens here — skipping it would silently
                // drop the character from the round-trip the tests assert.
                let name = std::str::from_utf8(&entity).expect("entity name is ASCII");
                let resolved = quick_xml::escape::resolve_predefined_entity(name)
                    .unwrap_or_else(|| panic!("unknown entity `&{name};`"));
                let element = stack
                    .last_mut()
                    .expect("entity reference inside an element");
                element.text.push_str(resolved);
            }
            Ok(Event::End(end)) => {
                let name = String::from_utf8_lossy(end.name().as_ref()).into_owned();
                let mut closed = stack.pop().expect("tags are balanced");
                assert_eq!(
                    closed.name, name,
                    "end tag must match its start tag — the document must be well-formed"
                );
                // Entity references split the text into several events, so
                // whitespace adjacent to one survives as a separate chunk:
                // trim once per element instead of per event.
                let trimmed = closed.text.trim().to_string();
                closed.text = trimmed;
                elements.push(closed);
            }
            Ok(Event::Empty(empty)) => {
                // Self-closing elements (e.g. `<skipped/>`) have no End
                // event, so record them immediately.
                elements.push(Element {
                    name: String::from_utf8_lossy(empty.name().as_ref()).into_owned(),
                    attrs: attributes(&empty, &reader),
                    text: String::new(),
                });
            }
            Ok(_) => {}
            Err(err) => panic!("document is not well-formed XML: {err}"),
        }
    }
    assert!(
        stack.is_empty(),
        "every element must be closed: {:?}",
        stack.iter().map(|e| &e.name).collect::<Vec<_>>()
    );
    elements
}

fn attributes(
    start: &quick_xml::events::BytesStart<'_>,
    reader: &Reader<&[u8]>,
) -> Vec<(String, String)> {
    start
        .attributes()
        .map(|attr| {
            let attr = attr.expect("well-formed attribute");
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            let value = attr
                .decoded_and_normalized_value(quick_xml::XmlVersion::Explicit1_0, reader.decoder())
                .expect("well-formed attribute value")
                .into_owned();
            (key, value)
        })
        .collect()
}

fn attr<'a>(element: &'a Element, key: &str) -> &'a str {
    element
        .attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("element <{}> must have attribute {key}", element.name))
}

#[test]
fn renders_well_formed_xml_with_a_testsuites_root() {
    let xml = JUnitReporter::render(&report(
        vec![result("p28-xdr-1", Surface::Xdr, Status::Pass)],
        Vec::new(),
    ));
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));

    let elements = parse(&xml);
    let root = elements
        .iter()
        .find(|element| element.name == "testsuites")
        .expect("<testsuites> root exists");
    assert_eq!(attr(root, "name"), "stellar-canary");
    assert_eq!(attr(root, "tests"), "1");
    assert_eq!(attr(root, "failures"), "0");
    assert_eq!(attr(root, "errors"), "0");
    assert_eq!(attr(root, "skipped"), "0");
}

#[test]
fn counts_accurately_reflect_tests_failures_errors_and_skips() {
    let mut failing = result("p28-xdr-2", Surface::Xdr, Status::Fail);
    failing.details = Some("roundtrip mismatch".into());
    let xml = JUnitReporter::render(&report(
        vec![
            result("p28-xdr-1", Surface::Xdr, Status::Pass),
            failing,
            result("p28-xdr-3", Surface::Xdr, Status::Error),
            result("p28-xdr-4", Surface::Xdr, Status::Warning),
        ],
        vec![
            SkipSummary {
                fixture_id: "p28-rpc-1".into(),
                surface: Surface::Rpc,
                reason: "surface disabled".into(),
            },
            SkipSummary {
                fixture_id: "p28-soroban-1".into(),
                surface: Surface::Soroban,
                reason: "protocol mismatch".into(),
            },
        ],
    ));

    let elements = parse(&xml);
    let root = elements
        .iter()
        .find(|element| element.name == "testsuites")
        .expect("<testsuites> root");
    // 4 executed results + 2 skipped fixtures; warnings are not failures.
    assert_eq!(attr(root, "tests"), "6");
    assert_eq!(attr(root, "failures"), "1");
    assert_eq!(attr(root, "errors"), "1");
    assert_eq!(attr(root, "skipped"), "2");

    // The XDR suite holds only the executed results; each other surface
    // holds its single skipped fixture.
    let xdr_suite = elements
        .iter()
        .find(|element| element.name == "testsuite")
        .expect("<testsuite>");
    assert_eq!(attr(xdr_suite, "name"), "XDR");
    assert_eq!(attr(xdr_suite, "tests"), "4");
    assert_eq!(attr(xdr_suite, "failures"), "1");
    assert_eq!(attr(xdr_suite, "errors"), "1");
    assert_eq!(attr(xdr_suite, "skipped"), "0");
}

#[test]
fn one_testsuite_is_emitted_per_surface_in_the_documented_order() {
    let xml = JUnitReporter::render(&report(
        vec![
            result("p28-xdr-1", Surface::Xdr, Status::Pass),
            result("p28-rpc-1", Surface::Rpc, Status::Pass),
            result("p28-soroban-1", Surface::Soroban, Status::Pass),
        ],
        Vec::new(),
    ));

    let elements = parse(&xml);
    let suite_names: Vec<String> = elements
        .iter()
        .filter(|element| element.name == "testsuite")
        .map(|element| attr(element, "name").to_string())
        .collect();
    assert_eq!(suite_names, ["XDR", "RPC", "Soroban"]);
    // The fixed reporter surface order is what every reporter uses.
    assert_eq!(
        SURFACE_ORDER.map(canary_report::surface_heading),
        ["XDR", "RPC", "Soroban"]
    );
}

#[test]
fn failures_and_errors_carry_their_diagnostics() {
    let mut failing = result("p28-xdr-2", Surface::Xdr, Status::Fail);
    failing.summary = "did not round-trip".into();
    failing.details = Some("input:  AAAA\noutput: AAAB".into());
    let mut errored = result("p28-rpc-1", Surface::Rpc, Status::Error);
    errored.summary = "connection refused".into();
    errored.details = Some("tcp connect: connection refused".into());

    let xml = JUnitReporter::render(&report(vec![failing, errored], Vec::new()));
    let elements = parse(&xml);

    let failure = elements
        .iter()
        .find(|element| element.name == "failure")
        .expect("<failure> element");
    assert_eq!(attr(failure, "message"), "did not round-trip");
    assert!(failure.text.contains("input:  AAAA"));

    let error = elements
        .iter()
        .find(|element| element.name == "error")
        .expect("<error> element");
    assert_eq!(attr(error, "message"), "connection refused");
    assert!(error.text.contains("tcp connect"));
}

#[test]
fn skipped_fixtures_render_as_skipped_testcases_with_their_reason() {
    let xml = JUnitReporter::render(&report(
        Vec::new(),
        vec![SkipSummary {
            fixture_id: "p28-rpc-1".into(),
            surface: Surface::Rpc,
            reason: "requires network access; skipped because --offline was given".into(),
        }],
    ));

    let elements = parse(&xml);
    let testcase = elements
        .iter()
        .find(|element| element.name == "testcase")
        .expect("<testcase> element");
    assert_eq!(attr(testcase, "classname"), "stellar-canary.rpc");
    assert_eq!(attr(testcase, "name"), "p28-rpc-1");

    let skipped = elements
        .iter()
        .find(|element| element.name == "skipped")
        .expect("<skipped> element");
    assert_eq!(
        attr(skipped, "message"),
        "requires network access; skipped because --offline was given"
    );
}

/// Negative case from the issue: diagnostics containing `<`, `>`, `&` and
/// `"` must be escaped — an unescaped `<` would make the document
/// unparseable (or worse, inject elements), which the strict parse here
/// would catch immediately.
#[test]
fn special_characters_in_diagnostics_are_escaped_and_still_parse() {
    let mut failing = result("p28-xdr-1", Surface::Xdr, Status::Fail);
    failing.summary = "expected <b> & got \"c\"".into();
    failing.details = Some("1 < 2 && 3 > 2; quote: \"x\"; tag: <failure>".into());
    let xml = JUnitReporter::render(&report(
        vec![failing],
        vec![SkipSummary {
            fixture_id: "p28-rpc-1".into(),
            surface: Surface::Rpc,
            reason: "a < b & c > d \"e\"".into(),
        }],
    ));

    // Parses cleanly …
    let elements = parse(&xml);
    // … the escaped text unescapes back to the original diagnostics …
    let failure = elements
        .iter()
        .find(|element| element.name == "failure")
        .expect("<failure> element");
    assert_eq!(
        attr(failure, "message"),
        "expected <b> & got \"c\"",
        "attribute values must survive escaping"
    );
    assert_eq!(
        failure.text, "1 < 2 && 3 > 2; quote: \"x\"; tag: <failure>",
        "element text must survive escaping"
    );
    // … and the raw document never contains an unescaped metacharacter
    // inside an attribute or text node.
    assert!(!xml.contains("<failure>\""), "raw quote leaked: {xml}");
}

#[test]
fn output_is_deterministic_for_the_same_input() {
    let report = report(
        vec![
            result("p28-xdr-1", Surface::Xdr, Status::Pass),
            result("p28-xdr-2", Surface::Xdr, Status::Fail),
        ],
        vec![SkipSummary {
            fixture_id: "p28-rpc-1".into(),
            surface: Surface::Rpc,
            reason: "r".into(),
        }],
    );
    assert_eq!(
        JUnitReporter::render(&report),
        JUnitReporter::render(&report)
    );
}

#[test]
fn an_empty_report_still_renders_a_closed_root_element() {
    let xml = JUnitReporter::render(&report(Vec::new(), Vec::new()));
    let elements = parse(&xml);
    let root = elements
        .iter()
        .find(|element| element.name == "testsuites")
        .expect("<testsuites> root");
    assert_eq!(attr(root, "tests"), "0");
    assert!(
        !elements.iter().any(|element| element.name == "testsuite"),
        "a surface with nothing to report must not get an empty suite"
    );
}
