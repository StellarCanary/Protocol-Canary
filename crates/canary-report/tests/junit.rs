//! JUnit XML export tests.
//!
//! These parse the rendered document with a real XML parser (`quick-xml`,
//! a dev-dependency used only here) instead of substring matching, so a
//! malformed document fails the suite rather than a lucky `contains`.

use canary_core::{
    CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, SkipCode,
    Status, Surface,
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
                    name: start.name().as_ref().to_owned(),
                    attrs: attributes(&start),
                    text: String::new(),
                });
            }
            Ok(Event::Text(text)) => {
                let text = text.xml10_content();
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
                let name: &str = &entity;
                let resolved = quick_xml::escape::resolve_predefined_entity(name)
                    .unwrap_or_else(|| panic!("unknown entity `&{name};`"));
                let element = stack
                    .last_mut()
                    .expect("entity reference inside an element");
                element.text.push_str(resolved);
            }
            Ok(Event::End(end)) => {
                let name = end.name().as_ref().to_owned();
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
                    name: empty.name().as_ref().to_owned(),
                    attrs: attributes(&empty),
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

fn attributes(start: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    start
        .attributes()
        .map(|attr| {
            let attr = attr.expect("well-formed attribute");
            let key = attr.key.as_ref().to_owned();
            let value = attr
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
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
                code: SkipCode::SurfaceDisabled,
            },
            SkipSummary {
                fixture_id: "p28-soroban-1".into(),
                surface: Surface::Soroban,
                reason: "protocol mismatch".into(),
                code: SkipCode::UnsupportedProtocol,
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
            code: SkipCode::Unknown,
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
            code: SkipCode::Unknown,
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
            code: SkipCode::Unknown,
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

/// Asserts that every character in the rendered XML string belongs to the
/// permitted XML 1.0 character set.
///
/// Under XML 1.0 (Fifth Edition):
/// `Char ::= #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]`
fn assert_valid_xml_10_characters(xml: &str) {
    for (i, c) in xml.chars().enumerate() {
        let cp = c as u32;
        let is_valid = matches!(
            cp,
            0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
        );
        assert!(
            is_valid,
            "XML contains character U+{cp:04X} forbidden by XML 1.0 at character position {i}: {c:?}"
        );
    }
}

#[test]
fn ansi_escape_sequences_and_cdata_closing_tokens_are_handled() {
    let mut failing = result("p28-xdr-ansi", Surface::Xdr, Status::Fail);
    failing.summary = "\x1b[31mError:\x1b[0m payload failed with ]]> in message".into();
    failing.details = Some(
        "Traceback:\n  \x1b[1;33mwarning\x1b[0m: nested CDATA <![CDATA[nested]]> token".into(),
    );

    let mut errored = result("p28-rpc-ansi", Surface::Rpc, Status::Error);
    errored.summary = "crash in <eval> with \x1b[32mcolor\x1b[0m and ]]>".into();
    errored.details = Some("details with \x1b[0m and ]]> end token".into());

    let xml = JUnitReporter::render(&report(vec![failing, errored], Vec::new()));

    // Verify all emitted characters conform strictly to XML 1.0
    assert_valid_xml_10_characters(&xml);
    assert!(
        !xml.contains('\x1b'),
        "raw ESC byte must not appear in rendered XML: {xml}"
    );
    assert!(
        xml.contains(r"\u{001B}[31mError:\u{001B}[0m"),
        "ESC must be represented deterministically as \\u{{001B}}: {xml}"
    );

    // Document must be strictly well-formed XML and parseable by standard XML parsers
    let elements = parse(&xml);

    let failure = elements
        .iter()
        .find(|element| element.name == "failure")
        .expect("<failure> element");
    assert!(
        attr(failure, "message").contains("]]>"),
        "attribute value must preserve ]]> unescaped"
    );
    assert!(
        attr(failure, "message").contains(r"\u{001B}[31mError:\u{001B}[0m"),
        "attribute value must carry safe \\u{{001B}} representation"
    );
    assert!(
        failure
            .text
            .contains("nested CDATA <![CDATA[nested]]> token"),
        "element text must preserve content"
    );
    assert!(
        failure.text.contains(r"\u{001B}[1;33mwarning\u{001B}[0m"),
        "element text must carry safe \\u{{001B}} representation"
    );

    let error = elements
        .iter()
        .find(|element| element.name == "error")
        .expect("<error> element");
    assert!(error.text.contains("]]> end token"));

    // Raw XML must escape '>' so ']]>' never appears unescaped as raw ']]>' inside element text
    assert!(
        !xml.contains("]]>"),
        "raw ]]> must never leak into rendered XML text: {xml}"
    );
    assert!(
        xml.contains("]]&gt;"),
        "nested CDATA close tokens must be escaped as ]]&gt;: {xml}"
    );
}

#[test]
fn forbidden_xml_control_characters_and_noncharacters_are_safely_encoded_and_valid() {
    let mut failing = result("p28-xdr-forbidden", Surface::Xdr, Status::Fail);
    // Include a mix of forbidden control characters, noncharacters U+FFFE/U+FFFF,
    // allowed whitespace (TAB, LF, CR), XML metacharacters (&, <, >, ", '), and CDATA end token (]]>).
    failing.summary = "bad chars: nul:\0, bell:\x07, bs:\x08, vt:\x0b, ff:\x0c, esc:\x1b, us:\x1f, nonchar:\u{FFFE} & \u{FFFF} <tag> 'quote' \"double\" ]]>".into();
    failing.details = Some(
        "diagnostics:\n\tline 1: \0 \x01 \x08 \x0b \x0c \x1b \x1f\r\n\tline 2: \u{FFFE} \u{FFFF} <script> & \"bar\" ]]>".into(),
    );

    let mut errored = result("p28-rpc-forbidden", Surface::Rpc, Status::Error);
    errored.summary = "error with \x1b and \0 and <meta>".into();
    errored.details = Some("errored \x1b[31mfailure\x1b[0m with \u{FFFF} and ]]>".into());

    let xml = JUnitReporter::render(&report(vec![failing, errored], Vec::new()));

    // 1. Every character in the rendered document must be valid XML 1.0
    assert_valid_xml_10_characters(&xml);

    // 2. The rendered XML must be parseable by standard XML parsers
    let elements = parse(&xml);

    // 3. Raw forbidden characters must not be in the output
    for forbidden in [
        '\0', '\x01', '\x07', '\x08', '\x0b', '\x0c', '\x1b', '\x1f', '\u{FFFE}', '\u{FFFF}',
    ] {
        assert!(
            !xml.contains(forbidden),
            "rendered XML must not contain forbidden character U+{:04X}",
            forbidden as u32
        );
    }

    // 4. Safe representations must be present
    assert!(
        xml.contains(r"\u{001B}"),
        "ESC must be represented as \\u{{001B}}"
    );
    assert!(
        xml.contains(r"\u{0000}"),
        "NUL must be represented as \\u{{0000}}"
    );
    assert!(
        xml.contains(r"\u{0007}"),
        "BEL must be represented as \\u{{0007}}"
    );
    assert!(
        xml.contains(r"\u{0008}"),
        "BS must be represented as \\u{{0008}}"
    );
    assert!(
        xml.contains(r"\u{000B}"),
        "VT must be represented as \\u{{000B}}"
    );
    assert!(
        xml.contains(r"\u{000C}"),
        "FF must be represented as \\u{{000C}}"
    );
    assert!(
        xml.contains(r"\u{001F}"),
        "US must be represented as \\u{{001F}}"
    );
    assert!(
        xml.contains(r"\u{FFFE}"),
        "U+FFFE must be represented as \\u{{FFFE}}"
    );
    assert!(
        xml.contains(r"\u{FFFF}"),
        "U+FFFF must be represented as \\u{{FFFF}}"
    );

    // 5. Permitted whitespace (TAB, LF, CR) and XML metacharacters are preserved/escaped properly
    assert!(xml.contains('\t'), "TAB (U+0009) must remain permitted");
    assert!(xml.contains('\n'), "LF (U+000A) must remain permitted");
    assert!(xml.contains('\r'), "CR (U+000D) must remain permitted");
    assert!(!xml.contains("<tag>"), "< must be escaped as &lt;");
    assert!(!xml.contains("<script>"), "< must be escaped as &lt;");
    assert!(
        !xml.contains("]]>"),
        "raw ]]> sequence must never appear in rendered XML"
    );
    assert!(
        xml.contains("]]&gt;"),
        "CDATA closing tokens must be escaped as ]]&gt;"
    );

    // 6. Parsed element text and attribute values retain escaped diagnostics cleanly
    let failure = elements
        .iter()
        .find(|e| e.name == "failure")
        .expect("<failure> element");
    assert!(attr(failure, "message").contains(r"\u{001B}"));
    assert!(attr(failure, "message").contains(r"\u{0000}"));
    assert!(attr(failure, "message").contains(r"\u{FFFF}"));
    assert!(attr(failure, "message").contains("]]>"));
    assert!(attr(failure, "message").contains("<tag>"));
    assert!(attr(failure, "message").contains("'quote'"));
    assert!(attr(failure, "message").contains("\"double\""));

    assert!(failure.text.contains(r"\u{001B}"));
    assert!(failure.text.contains(r"\u{0000}"));
    assert!(failure.text.contains(r"\u{FFFF}"));
    assert!(failure.text.contains("]]>"));
    assert!(failure.text.contains("<script>"));
    assert!(failure.text.contains('\t'));
    assert!(failure.text.contains('\n'));

    let error = elements
        .iter()
        .find(|e| e.name == "error")
        .expect("<error> element");
    assert!(attr(error, "message").contains(r"\u{001B}"));
    assert!(error.text.contains(r"\u{001B}[31mfailure\u{001B}[0m"));
    assert!(error.text.contains("]]>"));
}
