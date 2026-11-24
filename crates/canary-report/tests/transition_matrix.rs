//! The CF-05 golden corpus (issue #460): report pairs, one per state
//! transition, and the `diffVersion` 1 output each pair must produce.
//!
//! Three things are checked, and they are deliberately independent:
//!
//! 1. the transition table in `docs/contracts/cf-05-report-comparison.md`
//!    section 4, read out of the contract text, says which classification and
//!    which regression outcome every `(from, to)` pair must have;
//! 2. the corpus files under `tests/fixtures/diff/` hold expected outputs
//!    whose entries agree with that table, whose `missing`, `summary` and
//!    `verdict` agree with sections 4 and 5, and whose pairs cover every row
//!    of section 4 (`absent` to `absent` as far as a report pair can express
//!    it at all);
//! 3. `canary_report::diff::compare_json` reproduces those expected documents
//!    exactly — same values, same sorted order, same key order.
//!
//! (1) comes from the contract and (3) from the library, so neither side can
//! drift away from the other in silence. The Action and the viewer display this
//! same table (CF-05 section 8), which is why it is pinned here.
//!
//! The pairs are ordinary CF-01 reports with `git` and `verbose` omitted — both
//! are optional per CF-01 and irrelevant to a comparison — so each fixture
//! carries only what its case is about.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use canary_report::diff::{compare_json, CompareOptions, Ineligible, Side, MAX_REPORT_BYTES};
use serde_json::{json, Value};

const CONTRACT: &str = include_str!("../../../docs/contracts/cf-05-report-comparison.md");

/// The 12 classifications section 4 defines, in contract order.
const CLASSIFICATIONS: [&str; 12] = [
    "unchanged",
    "degraded",
    "regressed",
    "improved",
    "resolved",
    "added",
    "added-failing",
    "removed",
    "removed-failing",
    "skipped",
    "skipped-failing",
    "unchanged-skip",
];

/// The six states of section 4, `absent` included.
const STATES: [&str; 6] = ["pass", "warning", "fail", "error", "skipped", "absent"];

/// The top-level key order CF-05 section 5 fixes for `diffVersion` 1.
const DOCUMENT_KEYS: [&str; 12] = [
    "diffVersion",
    "comparable",
    "baseline",
    "current",
    "fixturePackChanged",
    "observedProtocolChanged",
    "emptyBaseline",
    "emptyCurrent",
    "entries",
    "missing",
    "summary",
    "verdict",
];

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("diff")
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&read_text(path)).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn as_str(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn as_usize(value: &Value) -> usize {
    value.as_u64().unwrap_or_default() as usize
}

fn as_bool(value: &Value) -> bool {
    value.as_bool().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The contract's own table
// ---------------------------------------------------------------------------

/// Section 4's third column: `no`, `yes`, or the `warnings_are_failures`
/// exception the `pass` to `warning` row carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CountsAsRegression {
    No,
    Yes,
    OnlyWithPolicy,
}

/// The section 4 table as a map: `(from, to)` to the classification that pair
/// must have and what the contract says about counting it as a regression.
type Transitions = HashMap<(String, String), (String, CountsAsRegression)>;

/// One parsed row of the section 4 table. A row may name several states on
/// either side, and one cell may hold more than one clause, which becomes more
/// than one row.
#[derive(Debug)]
struct Row {
    from: Vec<String>,
    to: Vec<String>,
    classification: String,
    policy: CountsAsRegression,
    /// The `X to X` row, which stands for the six diagonal pairs only.
    identity: bool,
}

/// The backticked tokens of one table cell, each paired with the text that
/// precedes it. Those separators are what tell `to` from `or`.
fn quoted_tokens(cell: &str) -> Vec<(String, String)> {
    let mut tokens = Vec::new();
    let mut rest = cell;
    let mut prefix = String::new();
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('`') else {
            break;
        };
        tokens.push((prefix.clone(), after[..end].to_string()));
        rest = &after[end + 1..];
        prefix = match rest.find('`') {
            Some(next) => rest[..next].to_string(),
            None => String::new(),
        };
    }
    tokens
}

/// Group one cell's state names into clauses: a separator containing `to`
/// starts the destination group, and a comma after a destination group has been
/// started, followed by another `to`, begins a new clause
/// (`` `skipped` to `absent`, `absent` to `skipped` `` is two rows of the table
/// written as one). A comma *before* the `to` is just a list of states
/// (`any of `pass`, `warning` to `skipped``).
fn split_clauses(cell: &str) -> Vec<(Vec<String>, Vec<String>)> {
    let tokens = quoted_tokens(cell);
    let mut clauses = Vec::new();
    let mut from: Vec<String> = Vec::new();
    let mut to: Vec<String> = Vec::new();
    for (index, (separator, token)) in tokens.iter().enumerate() {
        if index == 0 {
            from.push(token.clone());
            continue;
        }
        let next_starts_clause = tokens
            .get(index + 1)
            .is_some_and(|(next, _)| next.contains("to"));
        if separator.contains("to") {
            to.push(token.clone());
        } else if separator.contains(',') && !to.is_empty() && next_starts_clause {
            clauses.push((std::mem::take(&mut from), std::mem::take(&mut to)));
            from.push(token.clone());
        } else if to.is_empty() {
            from.push(token.clone());
        } else {
            to.push(token.clone());
        }
    }
    if !from.is_empty() {
        clauses.push((from, to));
    }
    clauses
}

/// Every row of the section 4 transition table, read out of the contract text.
fn contract_rows() -> Vec<Row> {
    let doc = CONTRACT.replace("\r\n", "\n");
    let start = doc
        .find("## 4. States and transitions")
        .expect("CF-05 has a section 4");
    let end = doc[start..]
        .find("**Missing checks.**")
        .map(|offset| start + offset)
        .expect("section 4 ends at the missing-checks note");
    let mut rows = Vec::new();
    for line in doc[start..end]
        .lines()
        .filter(|line| line.starts_with("| "))
    {
        let cells = <[String; 3]>::try_from(
            line.trim()
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|cells| panic!("section 4 row has {} cells: {line}", cells.len()));
        let [states_cell, classification_cell, policy_cell] = cells;
        if states_cell == "Baseline to current" {
            continue;
        }
        let classification = quoted_tokens(&classification_cell)
            .into_iter()
            .map(|(_, token)| token)
            .next()
            .unwrap_or_else(|| panic!("no classification in row {line}"));
        let policy = if policy_cell.starts_with("yes") {
            CountsAsRegression::Yes
        } else if policy_cell.contains("unless policy") {
            CountsAsRegression::OnlyWithPolicy
        } else {
            CountsAsRegression::No
        };
        let clauses = if states_cell == "X to X" {
            // The identity row: every state against itself, and nothing else.
            vec![(STATES.map(String::from).to_vec(), Vec::new())]
        } else {
            let clauses = split_clauses(&states_cell);
            for clause in &clauses {
                if clause.0.is_empty() || clause.1.is_empty() {
                    panic!("CF-05 section 4 row {states_cell} names both states of a transition");
                }
            }
            clauses
        };
        let identity = states_cell == "X to X";
        rows.extend(clauses.into_iter().map(|(from, to)| Row {
            from,
            to,
            classification: classification.clone(),
            policy,
            identity,
        }));
    }
    rows
}

/// `(from, to)` -> (classification, counts as regression). Rows are applied in
/// the contract's order, and the identity row's reading of a diagonal pair is
/// never replaced; [`transition_table_and_refinements`] reports any pair that
/// two rows claim, which the suite treats as a broken contract.
fn transition_table() -> Transitions {
    transition_table_and_refinements().0
}

/// The section 4 table as a map, plus the list of pairs a second row re-reads.
fn transition_table_and_refinements() -> (Transitions, Vec<String>) {
    let mut table = HashMap::new();
    let mut refinements = Vec::new();
    for row in contract_rows() {
        if row.identity {
            // The `X to X` row is the six diagonal pairs and nothing else.
            for state in &row.from {
                table.insert(
                    (state.clone(), state.clone()),
                    (row.classification.clone(), row.policy),
                );
            }
            continue;
        }
        for from in &row.from {
            for to in &row.to {
                let pair = (from.clone(), to.clone());
                let entry = (row.classification.clone(), row.policy);
                // A group row can still name a diagonal pair; the identity
                // reading is the one CF-05 means, so it is never replaced.
                if from == to {
                    table.entry(pair).or_insert(entry);
                    continue;
                }
                if let Some(previous) = table.insert(pair.clone(), entry.clone()) {
                    refinements.push(format!(
                        "{from} to {to}: {} replaced by {}",
                        previous.0, row.classification
                    ));
                }
            }
        }
    }
    (table, refinements)
}

// ---------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------

/// One case: its name, the pair it compares, the options, and either the
/// expected document or the refusal the pair must produce.
struct Case {
    name: String,
    pair: String,
    options: CompareOptions,
    expected: Option<Value>,
    expected_error: Option<Value>,
    path: PathBuf,
}

impl Case {
    fn baseline_text(&self) -> String {
        read_text(
            &corpus_dir()
                .join("pairs")
                .join(format!("{}.baseline.json", self.pair)),
        )
    }

    fn current_text(&self) -> String {
        read_text(
            &corpus_dir()
                .join("pairs")
                .join(format!("{}.current.json", self.pair)),
        )
    }

    /// The engine omits `sourceChanged` when neither side was cached while the
    /// corpus writes it as an explicit `null`, so every entry has the same keys.
    fn normalized(&self, value: Value) -> Value {
        let mut value = value;
        for entry in value["entries"]
            .as_array_mut()
            .expect("a comparison has entries")
            .iter_mut()
        {
            entry
                .as_object_mut()
                .expect("an entry is an object")
                .entry("sourceChanged")
                .or_insert(Value::Null);
        }
        value
    }

    fn compare(&self) -> Result<Value, Ineligible> {
        compare_json(&self.baseline_text(), &self.current_text(), self.options)
            .map(|comparison| self.normalized(json!(comparison)))
    }

    /// The comparison this case's *inputs* produce, with one side's `results`
    /// and `skipped` lists reversed. CF-05 section 7: shuffling the input
    /// changes nothing about the output.
    fn compare_shuffled(&self) -> Result<Value, Ineligible> {
        let shuffle = |text: &str| -> String {
            let mut value: Value = serde_json::from_str(text).expect("a corpus report is JSON");
            let object = value.as_object_mut().expect("a corpus report is an object");
            for key in ["results", "skipped"] {
                if let Some(list) = object.get_mut(key).and_then(Value::as_array_mut) {
                    list.reverse();
                }
            }
            // `counts` is a rendering of the same results in their old order,
            // and comparison never reads it.
            object.remove("counts");
            serde_json::to_string(&value).expect("reserializes")
        };
        let comparison = compare_json(
            &shuffle(&self.baseline_text()),
            &shuffle(&self.current_text()),
            self.options,
        )?;
        Ok(self.normalized(json!(comparison)))
    }
}

fn cases() -> Vec<Case> {
    let index = read_json(&corpus_dir().join("cases.json"))["cases"]
        .as_array()
        .expect("cases.json lists its cases")
        .iter()
        .map(|name| as_str(name).to_string())
        .collect::<Vec<_>>();
    assert!(!index.is_empty(), "cases.json must name at least one case");
    index
        .into_iter()
        .map(|name| {
            let path = corpus_dir()
                .join("cases")
                .join(format!("{name}.expected.json"));
            let document = read_json(&path);
            assert_eq!(
                as_str(&document["case"]),
                name,
                "{} names its own case",
                path.display()
            );
            Case {
                expected: document.get("expected").cloned(),
                name,
                pair: as_str(&document["pair"]).to_string(),
                options: CompareOptions {
                    warnings_are_failures: as_bool(&document["options"]["warningsAreFailures"]),
                },
                expected_error: document.get("expectedError").cloned(),
                path,
            }
        })
        .collect()
}

/// The classification, regression outcome, `missing` set, summary buckets,
/// verdict and key set that sections 4 and 5 prescribe for one expected
/// document — derived from the contract table, never from the library.
/// Returns the disagreements, so the same function can fail a tampered
/// document in the negative test.
fn disagreements_with_the_contract(
    expected: &Value,
    options: CompareOptions,
    table: &Transitions,
) -> Vec<String> {
    let mut problems = Vec::new();
    let entries = expected["entries"]
        .as_array()
        .expect("an expected document has entries");
    // CF-05 section 2: an empty side cannot establish a regression.
    let empty_side = as_bool(&expected["emptyBaseline"]) || as_bool(&expected["emptyCurrent"]);

    for entry in entries {
        let from = as_str(&entry["from"]).to_string();
        let to = as_str(&entry["to"]).to_string();
        let Some((classification, policy)) = table.get(&(from.clone(), to.clone())) else {
            problems.push(format!("{from} to {to} is not a row of CF-05 section 4"));
            continue;
        };
        let stated = as_str(&entry["classification"]);
        if stated != classification {
            problems.push(format!(
                "CF-05 section 4 makes {from} to {to} {classification}, the corpus says {stated}"
            ));
        }
        let regression = match policy {
            CountsAsRegression::Yes => true,
            CountsAsRegression::OnlyWithPolicy => options.warnings_are_failures,
            CountsAsRegression::No => false,
        } && !empty_side;
        if as_bool(&entry["regression"]) != regression {
            problems.push(format!(
                "{from} to {to} must have regression {regression} under \
                 warnings_are_failures={}",
                options.warnings_are_failures
            ));
        }
    }

    let identity = |entry: &Value| {
        (
            as_str(&entry["surface"]).to_string(),
            as_str(&entry["id"]).to_string(),
        )
    };
    let missing: Vec<(String, String)> = entries
        .iter()
        .filter(|entry| {
            matches!(as_str(&entry["from"]), "pass" | "warning")
                && matches!(as_str(&entry["to"]), "absent" | "skipped")
        })
        .map(identity)
        .collect();
    let stated_missing: Vec<(String, String)> = expected["missing"]
        .as_array()
        .expect("an expected document has missing")
        .iter()
        .map(identity)
        .collect();
    if missing != stated_missing {
        problems.push(format!(
            "missing should be {missing:?}, the corpus says {stated_missing:?}"
        ));
    }

    let bucket = |names: &[&str]| {
        entries
            .iter()
            .filter(|entry| names.contains(&as_str(&entry["classification"])))
            .count()
    };
    let summary = &expected["summary"];
    for (key, value) in [
        (
            "regressed",
            entries.iter().filter(|e| as_bool(&e["regression"])).count(),
        ),
        ("resolved", bucket(&["resolved"])),
        ("added", bucket(&["added", "added-failing"])),
        ("removed", bucket(&["removed", "removed-failing"])),
        ("unchanged", bucket(&["unchanged", "unchanged-skip"])),
        ("missing", missing.len()),
    ] {
        if as_usize(&summary[key]) != value {
            problems.push(format!(
                "summary.{key} should be {value}, the corpus says {}",
                summary[key]
            ));
        }
    }

    // CF-05 section 5: first match wins, in this order.
    let verdict = if entries.iter().any(|e| as_bool(&e["regression"])) {
        "regression"
    } else if !missing.is_empty() {
        "coverage-lost"
    } else if entries
        .iter()
        .any(|e| ["resolved", "improved", "added"].contains(&as_str(&e["classification"])))
    {
        "improved"
    } else {
        "no-change"
    };
    if as_str(&expected["verdict"]) != verdict {
        problems.push(format!(
            "verdict should be {verdict} by CF-05 section 5's precedence, the corpus says {}",
            expected["verdict"]
        ));
    }

    for key in DOCUMENT_KEYS {
        if expected.get(key).is_none() {
            problems.push(format!("the expected document is missing {key}"));
        }
    }
    if as_usize(&expected["diffVersion"]) != 1 || !as_bool(&expected["comparable"]) {
        problems.push("a comparable corpus document is diffVersion 1".to_string());
    }
    problems
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

#[test]
fn the_contract_table_names_every_state_pair() {
    assert_eq!(
        contract_rows().len(),
        17,
        "section 4 writes 16 rows and one of them (`skipped` to `absent`, \
         `absent` to `skipped`) is two clauses"
    );
    let (table, refinements) = transition_table_and_refinements();
    for from in STATES {
        for to in STATES {
            assert!(
                table.contains_key(&(from.to_string(), to.to_string())),
                "CF-05 section 4 has no row for {from} to {to}"
            );
        }
    }
    // Apart from the identity row, no pair may be claimed by two rows: section 4
    // is a partition of the table, and a second reading of one pair would be
    // resolved here by row order, which is not how the contract reads.
    assert!(
        refinements.is_empty(),
        "CF-05 section 4 states a transition twice: {refinements:?}"
    );
    let classifications: BTreeSet<&str> = CLASSIFICATIONS.iter().copied().collect();
    assert_eq!(
        table
            .values()
            .map(|(classification, _)| classification.as_str())
            .collect::<BTreeSet<&str>>(),
        classifications,
        "the table must use exactly the 12 classifications CF-05 names"
    );
}

#[test]
fn every_state_pair_except_absent_to_absent_has_a_golden_case() {
    let table = transition_table();
    let corpus: BTreeSet<(String, String)> = cases()
        .iter()
        .filter_map(|case| case.expected.as_ref())
        .flat_map(|expected| expected["entries"].as_array().expect("entries").iter())
        .map(|entry| {
            (
                as_str(&entry["from"]).to_string(),
                as_str(&entry["to"]).to_string(),
            )
        })
        .collect();

    let uncovered: Vec<String> = STATES
        .iter()
        .flat_map(|from| STATES.iter().map(move |to| (*from, *to)))
        .filter(|(from, to)| !(*from == "absent" && *to == "absent"))
        .filter(|pair| !corpus.contains(&(pair.0.to_string(), pair.1.to_string())))
        .map(|(from, to)| format!("{from} to {to}"))
        .collect();
    assert!(
        uncovered.is_empty(),
        "the corpus has no golden case for: {uncovered:?}"
    );
    // An identity absent from both reports is in neither `results` nor
    // `skipped`, so no pair can produce an entry for it; `classify` covers that
    // row in its own unit test.
    assert!(
        !corpus.contains(&(String::from("absent"), String::from("absent"))),
        "absent to absent is not a comparison entry"
    );
    for pair in &corpus {
        assert!(
            table.contains_key(pair),
            "the corpus pairs {} to {}, which CF-05 section 4 does not name",
            pair.0,
            pair.1
        );
    }
}

#[test]
fn every_expected_document_agrees_with_the_contract() {
    let table = transition_table();
    let all = cases();
    let mut problems: Vec<String> = Vec::new();
    for case in &all {
        let Some(expected) = &case.expected else {
            continue;
        };
        for problem in disagreements_with_the_contract(expected, case.options, &table) {
            problems.push(format!("{}: {problem}", case.path.display()));
        }
    }
    assert!(
        problems.is_empty(),
        "the corpus and the contract disagree:\n{}",
        problems.join("\n")
    );
}

#[test]
fn an_invalid_combination_fails_with_a_descriptive_error() {
    let table = transition_table();
    let case = cases()
        .into_iter()
        .find(|case| case.name == "from-pass")
        .expect("the corpus has a from-pass case");
    let options = case.options;
    let tampered = {
        let mut document = case.expected.expect("from-pass is comparable");
        let entry = document["entries"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| as_str(entry.get("id").unwrap_or(&Value::Null)) == "pass-to-fail")
            .expect("from-pass compares pass to fail");
        *entry = json!({
            "surface": "xdr",
            "id": "pass-to-fail",
            "from": "pass",
            "to": "fail",
            "classification": "improved",
            "regression": false,
            "sourceChanged": null,
        });
        document
    };
    let problems = disagreements_with_the_contract(&tampered, options, &table);
    assert_eq!(
        problems.len(),
        3,
        "one wrong classification, one wrong regression, one wrong summary: {problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("pass to fail") && problem.contains("regressed")),
        "the failure must name the pair and the contract's reading: {problems:?}"
    );
}

#[test]
fn each_case_reproduces_its_expected_document() {
    for case in cases() {
        let Some(expected) = &case.expected else {
            continue;
        };
        let actual = case
            .compare()
            .unwrap_or_else(|e| panic!("{}: the pair was refused: {e}", case.name));
        assert_eq!(
            actual, *expected,
            "case {} differs from its expected document",
            case.name
        );
        assert_eq!(
            case.compare_shuffled().expect("still comparable"),
            *expected,
            "{}: the order of `results` leaked into the output",
            case.name
        );
    }
}

#[test]
fn the_engine_writes_the_contracts_key_order() {
    let doc = CONTRACT.replace("\r\n", "\n");
    let marker = "<!-- contract-example: diff-v1 -->";
    let at = doc.find(marker).expect("CF-05 has a diff-v1 example");
    let block = &doc[at..];
    let opened = block.find("```json").expect("the example is a json block") + "```json".len();
    let closed = opened + block[opened..].find("```").expect("the block closes");
    let body = &block[opened..closed];
    let documented: Vec<String> = body
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  \"")?;
            rest.split("\":").next().map(String::from)
        })
        .collect();
    assert_eq!(
        documented,
        DOCUMENT_KEYS
            .iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>(),
        "the contract example's top-level keys are the order CF-05 fixes"
    );

    let case = cases()
        .into_iter()
        .find(|case| case.name == "regressions")
        .expect("the corpus has a regressions case");
    let text = compare_json(&case.baseline_text(), &case.current_text(), case.options)
        .expect("comparable")
        .to_json();
    let positions: Vec<usize> = documented
        .iter()
        .map(|key| {
            text.find(&format!("\"{key}\""))
                .unwrap_or_else(|| panic!("the engine dropped {key}: {text}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the engine keys are out of contract order: {text}"
    );
    // Which keys exist is pinned by the expected documents themselves, which
    // carry all 12 of them for every case: `each_case_reproduces_its_expected_document`
    // compares whole objects, so an extra or dropped key fails there.
    assert_eq!(DOCUMENT_KEYS.len(), 12);
}

#[test]
fn entries_are_sorted_by_surface_then_id_in_utf8_byte_order() {
    let case = cases()
        .into_iter()
        .find(|case| case.name == "identity-ordering")
        .expect("the corpus has an identity-ordering case");
    let actual = case.compare().expect("comparable");
    let identities: Vec<Vec<String>> = actual["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| {
            vec![
                as_str(&entry["surface"]).to_string(),
                as_str(&entry["id"]).to_string(),
            ]
        })
        .collect();
    let expected: Vec<Vec<String>> = [
        ("rpc", "Zebra"),
        ("rpc", "apple"),
        ("rpc", "café"),
        ("soroban", "a"),
        ("xdr", "A"),
        ("xdr", "B"),
        ("xdr", "b"),
        ("xdr", "café"),
    ]
    .iter()
    .map(|(surface, id)| vec![surface.to_string(), id.to_string()])
    .collect();
    assert_eq!(identities, expected);
}

#[test]
fn the_corpus_baseline_for_v0_1_1_is_the_released_golden_report() {
    // CF-05 section 7: the corpus must carry a real v0.1.1 report as a
    // baseline, not a paraphrase of one.
    let corpus = read_text(
        &corpus_dir()
            .join("pairs")
            .join("v0-1-1-baseline.baseline.json"),
    );
    let released = read_text(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("golden")
            .join("v0.1.1-multi.json"),
    );
    assert_eq!(corpus, released, "the v0.1.1 baseline drifted");
    let case = cases()
        .into_iter()
        .find(|case| case.name == "v0-1-1-baseline")
        .expect("the corpus has a v0-1-1-baseline case");
    let expected = case.expected.as_ref().expect("comparable");
    assert_eq!(as_str(&expected["baseline"]["toolVersion"]), "0.1.1");
    assert_eq!(as_str(&expected["current"]["toolVersion"]), "0.2.0");
    assert_eq!(as_str(&expected["verdict"]), "regression");
}

#[test]
fn every_corpus_report_is_internally_consistent() {
    let mut stems: BTreeSet<String> = BTreeSet::new();
    for case in cases() {
        stems.insert(case.pair.clone());
        if case.expected_error.is_some() {
            continue;
        }
        for (side, text) in [
            ("baseline", case.baseline_text()),
            ("current", case.current_text()),
        ] {
            let report: Value =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} {side}: {e}", case.name));
            let results = report["results"].as_array().expect("results");
            let skipped = report
                .get("skipped")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let count_of = |status: &str| {
                results
                    .iter()
                    .filter(|result| as_str(&result["status"]) == status)
                    .count()
            };
            let counts = &report["counts"];
            for (key, value) in [
                ("total", results.len()),
                ("passed", count_of("pass")),
                ("failed", count_of("fail")),
                ("warnings", count_of("warning")),
                ("errors", count_of("error")),
                ("skipped", skipped.len()),
            ] {
                assert_eq!(
                    as_usize(&counts[key]),
                    value,
                    "{} {side}: counts.{key}",
                    case.name
                );
            }
            let worst = ["error", "fail", "warning"]
                .iter()
                .find(|status| count_of(status) > 0)
                .copied()
                .unwrap_or("pass");
            assert_eq!(
                as_str(&report["status"]),
                worst,
                "{} {side}: status must name the worst result outcome, or the \
                 comparison header reads a different status from the report",
                case.name
            );
            let identities: BTreeSet<(String, String)> = results
                .iter()
                .map(|result| {
                    (
                        as_str(&result["surface"]).to_string(),
                        result
                            .get("fixtureId")
                            .map(|id| as_str(id).to_string())
                            .unwrap_or_else(|| as_str(&result["testId"]).to_string()),
                    )
                })
                .chain(skipped.iter().map(|entry| {
                    (
                        as_str(&entry["surface"]).to_string(),
                        as_str(&entry["fixtureId"]).to_string(),
                    )
                }))
                .collect();
            assert_eq!(
                identities.len(),
                results.len() + skipped.len(),
                "{} {side}: repeats an identity, which CF-05 section 7 rejects \
                 before comparison",
                case.name
            );
        }
    }
    let on_disk: BTreeSet<String> = std::fs::read_dir(corpus_dir().join("pairs"))
        .expect("the corpus has a pairs directory")
        .map(|entry| entry.expect("a readable dir entry").path())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_string();
            name.strip_suffix(".baseline.json")
                .or_else(|| name.strip_suffix(".current.json"))
                .map(String::from)
        })
        .collect();
    assert_eq!(
        on_disk, stems,
        "a report pair is on disk but unreferenced, or a case has no pair"
    );
    let expected_files: BTreeSet<String> = std::fs::read_dir(corpus_dir().join("cases"))
        .expect("the corpus has a cases directory")
        .map(|entry| entry.expect("a readable dir entry").path())
        .filter_map(|path| {
            path.file_name()?
                .to_str()?
                .strip_suffix(".expected.json")
                .map(String::from)
        })
        .collect();
    assert_eq!(
        expected_files,
        cases()
            .into_iter()
            .map(|case| case.name)
            .collect::<BTreeSet<String>>(),
        "a case file is on disk but unlisted in cases.json, or a listed case \
         has no file"
    );
}

#[test]
fn a_fixture_pack_change_is_still_always_unknown() {
    // CF-05 section 2 can only answer this once reports carry `fixturePack`,
    // which CF-01 reserves but does not emit. The corpus pins `null` so adding
    // the field is a deliberate change here rather than a silent one.
    for case in cases() {
        let Some(expected) = &case.expected else {
            continue;
        };
        assert_eq!(expected["fixturePackChanged"], Value::Null, "{}", case.name);
        assert_eq!(
            case.compare().expect("comparable")["fixturePackChanged"],
            Value::Null,
            "{}",
            case.name
        );
    }
}

#[test]
fn an_ineligible_pair_fails_with_the_message_the_case_names() {
    for case in cases() {
        let Some(stated) = &case.expected_error else {
            continue;
        };
        let err = case
            .compare()
            .expect_err(&format!("{} must be refused", case.name));
        let (variant, side) = match &err {
            Ineligible::InvalidReport { side, .. } => ("InvalidReport", Some(*side)),
            Ineligible::TooLarge { side, .. } => ("TooLarge", Some(*side)),
            Ineligible::SchemaVersion { side, .. } => ("SchemaVersion", Some(*side)),
            Ineligible::TargetProtocolDiffers { .. } => ("TargetProtocolDiffers", None),
            Ineligible::DuplicateIdentity { side, .. } => ("DuplicateIdentity", Some(*side)),
        };
        assert_eq!(
            stated["variant"].as_str(),
            Some(variant),
            "{} is refused with {err}",
            case.name
        );
        assert_eq!(
            stated["side"].as_str(),
            side.map(|side| side.to_string()).as_deref(),
            "{} names the side at fault",
            case.name
        );
        let message = err.to_string();
        for fragment in stated["contains"]
            .as_array()
            .expect("expectedError names the fragments its message must carry")
        {
            let fragment = as_str(fragment);
            assert!(
                message.contains(fragment),
                "{}: {message} does not mention {fragment:?}",
                case.name
            );
        }
    }
}

#[test]
fn a_report_over_the_size_cap_is_refused_before_it_is_parsed() {
    // D-06's 10 MiB cap is a property of the loader, not a fixture: a corpus
    // file that big would swamp the rest of the repository.
    let oversized = " ".repeat(MAX_REPORT_BYTES + 1);
    let case = cases()
        .into_iter()
        .find(|case| case.name == "regressions")
        .expect("the corpus has a regressions case");
    let good = case.current_text();
    for (side, fault) in [("baseline", Side::Baseline), ("current", Side::Current)] {
        let (first, second) = if side == "baseline" {
            (&oversized, &good)
        } else {
            (&good, &oversized)
        };
        let err = compare_json(first, second, CompareOptions::default()).unwrap_err();
        let message = err.to_string();
        match err {
            Ineligible::TooLarge {
                side: at_fault,
                size,
            } => {
                assert_eq!(at_fault, fault);
                assert!(size > MAX_REPORT_BYTES);
                assert!(message.contains("over the"), "{message}");
            }
            other => panic!("an oversized {side} report must be TooLarge, got {other}"),
        }
    }
}

#[test]
fn a_duplicate_identity_is_rejected_before_comparison() {
    let case = cases()
        .into_iter()
        .find(|case| case.name == "duplicate-identity")
        .expect("the corpus has a duplicate-identity case");
    let err = case.compare().unwrap_err();
    assert!(
        matches!(
            err,
            Ineligible::DuplicateIdentity {
                side: Side::Baseline,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn the_policy_is_the_only_difference_between_the_degradation_twins() {
    let default = cases()
        .into_iter()
        .find(|case| case.name == "policy-degradation-default")
        .expect("the corpus has a default-policy case");
    let strict = cases()
        .into_iter()
        .find(|case| case.name == "policy-degradation-strict")
        .expect("the corpus has a strict-policy case");
    assert_eq!(
        default.pair, strict.pair,
        "both policies compare one report pair"
    );
    let without = default.expected.as_ref().expect("comparable");
    let with = strict.expected.as_ref().expect("comparable");
    for (document, regression) in [(without, false), (with, true)] {
        let entry = &document["entries"][0];
        assert_eq!(as_str(&entry["classification"]), "degraded");
        assert_eq!(as_bool(&entry["regression"]), regression);
        assert_eq!(
            as_str(&document["verdict"]),
            if regression {
                "regression"
            } else {
                "no-change"
            }
        );
        assert_eq!(
            as_usize(&document["summary"]["regressed"]),
            usize::from(regression)
        );
    }
    // Everything else about the two documents is identical: the policy changes
    // the reading of the transition, never the transition.
    let mut retagged = with.clone();
    retagged["entries"][0]["regression"] = Value::Bool(false);
    retagged["verdict"] = Value::String("no-change".into());
    retagged["summary"]["regressed"] = Value::from(0);
    assert_eq!(retagged, *without);
}

#[test]
fn an_empty_side_never_reports_a_regression() {
    for (name, flag) in [
        ("empty-baseline", "emptyBaseline"),
        ("empty-current", "emptyCurrent"),
    ] {
        let case = cases()
            .into_iter()
            .find(|case| case.name == name)
            .unwrap_or_else(|| panic!("the corpus has a {name} case"));
        let expected = case.expected.as_ref().expect("comparable");
        assert_eq!(
            expected[flag],
            Value::Bool(true),
            "{name} says which side is empty"
        );
        assert!(
            expected["entries"]
                .as_array()
                .expect("entries")
                .iter()
                .all(|entry| !as_bool(&entry["regression"])),
            "{name} must not call anything a regression"
        );
        assert_eq!(as_usize(&expected["summary"]["regressed"]), 0);
        assert_eq!(
            case.compare().expect("an empty side is comparable"),
            *expected,
            "{name}"
        );
    }
}

#[test]
fn verdict_precedence_puts_regression_above_coverage_above_improvement() {
    let precedence = cases()
        .into_iter()
        .find(|case| case.name == "verdict-precedence")
        .expect("the corpus has a verdict-precedence case");
    let document = precedence.expected.as_ref().expect("comparable");
    // The case holds one of each outcome at once, so only the precedence can
    // explain its verdict.
    assert_eq!(as_usize(&document["summary"]["regressed"]), 1);
    assert!(!document["missing"].as_array().expect("missing").is_empty());
    assert_eq!(as_usize(&document["summary"]["resolved"]), 1);
    assert_eq!(as_str(&document["verdict"]), "regression");

    let lost = cases()
        .into_iter()
        .find(|case| case.name == "missing-and-added")
        .expect("the corpus has a missing-and-added case");
    let lost_document = lost.expected.as_ref().expect("comparable");
    assert_eq!(as_usize(&lost_document["summary"]["added"]), 1);
    assert_eq!(as_str(&lost_document["verdict"]), "coverage-lost");
    assert_eq!(
        as_str(&lost.compare().expect("comparable")["verdict"]),
        "coverage-lost"
    );

    let verdicts: BTreeSet<String> = cases()
        .iter()
        .filter_map(|case| case.expected.as_ref())
        .map(|expected| as_str(&expected["verdict"]).to_string())
        .collect();
    assert_eq!(
        verdicts,
        ["regression", "coverage-lost", "improved", "no-change"]
            .iter()
            .map(|verdict| verdict.to_string())
            .collect::<BTreeSet<String>>(),
        "the corpus must exercise all four verdicts"
    );
}

// ---------------------------------------------------------------------------
// Skipped <-> failing transitions (issue #745)
// ---------------------------------------------------------------------------
//
// The golden corpus above already carries `from-skipped` and the `*-to-
// skipped` entries; these tests drive the same transitions through
// `diff::compare` with in-memory reports, one identity per pair, so the
// categorisation of a skip that starts failing ("added-failing") and of a
// failure that becomes a skip ("skipped-failing") is pinned explicitly, along
// with the summary buckets and verdict those entries must produce.

use canary_core::{
    CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, ResultSource,
    SkipCode, Status, Surface,
};
use canary_report::diff::{classify, compare, Classification, Comparison, State, Verdict};
use canary_report::{ProjectSummary, ReportInput, SkipSummary};

/// The CF-05 spelling of a state, as the contract table and entries use it.
fn state_name(state: State) -> String {
    as_str(&json!(state)).to_string()
}

fn classification_name(classification: Classification) -> String {
    as_str(&json!(classification)).to_string()
}

const ALL_STATES: [State; 6] = [
    State::Pass,
    State::Warning,
    State::Fail,
    State::Error,
    State::Skipped,
    State::Absent,
];

/// Puts identity `id` into `report` in `state`: a result for an outcome, a
/// skip-list entry for `skipped`, nothing for `absent`.
fn place(report: &mut ReportInput, id: &str, state: State) {
    let status = match state {
        State::Pass => Status::Pass,
        State::Warning => Status::Warning,
        State::Fail => Status::Fail,
        State::Error => Status::Error,
        State::Skipped => {
            report.skipped.push(SkipSummary {
                fixture_id: id.to_string(),
                surface: Surface::Xdr,
                reason: "fixture targets protocol 29, this run targets protocol 28".to_string(),
                code: SkipCode::UnsupportedProtocol,
            });
            return;
        }
        State::Absent => return,
    };
    report.results.push(CompatibilityResult {
        test_id: id.to_string(),
        protocol: ProtocolVersion(28),
        surface: Surface::Xdr,
        status,
        summary: String::new(),
        details: None,
        duration_ms: 1,
        fixture_id: Some(id.to_string()),
        source: ResultSource::Live,
    });
}

fn empty_report() -> ReportInput {
    ReportInput {
        tool_version: "0.3.0".into(),
        target_protocol: ProtocolVersion(28),
        project: ProjectSummary {
            name: "transition-matrix".into(),
            project_type: ProjectType::Soroban,
        },
        network: None,
        results: Vec::new(),
        skipped: Vec::new(),
        decision: PolicyDecision::Pass,
        git: GitContext::default(),
        verbose: false,
        verification: Vec::new(),
        run: None,
    }
}

/// A report pair holding one identity per `(from, to)` transition, plus an
/// `anchor` that passes on both sides so neither side is empty (an empty
/// side suppresses every regression, CF-05 section 2).
fn pair_of(transitions: &[(State, State)]) -> (ReportInput, ReportInput) {
    let mut baseline = empty_report();
    let mut current = empty_report();
    place(&mut baseline, "anchor", State::Pass);
    place(&mut current, "anchor", State::Pass);
    for (from, to) in transitions {
        let id = format!("{}-to-{}", state_name(*from), state_name(*to));
        place(&mut baseline, &id, *from);
        place(&mut current, &id, *to);
    }
    (baseline, current)
}

fn compare_pair(transitions: &[(State, State)], warnings_are_failures: bool) -> Comparison {
    let (baseline, current) = pair_of(transitions);
    compare(
        &baseline,
        &current,
        CompareOptions {
            warnings_are_failures,
        },
    )
    .expect("a pair with unique identities is comparable")
}

fn entry(comparison: &Comparison, from: State, to: State) -> &canary_report::diff::Entry {
    let id = format!("{}-to-{}", state_name(from), state_name(to));
    comparison
        .entries
        .iter()
        .find(|entry| entry.id == id)
        .unwrap_or_else(|| panic!("no entry for {id}"))
}

/// The skip-related rows of CF-05 section 4: every pair with `skipped` on
/// either side, other than the identity row.
const SKIP_TRANSITIONS: [(State, State, Classification, bool); 9] = [
    (
        State::Skipped,
        State::Fail,
        Classification::AddedFailing,
        true,
    ),
    (
        State::Skipped,
        State::Error,
        Classification::AddedFailing,
        true,
    ),
    (
        State::Fail,
        State::Skipped,
        Classification::SkippedFailing,
        true,
    ),
    (
        State::Error,
        State::Skipped,
        Classification::SkippedFailing,
        true,
    ),
    (State::Pass, State::Skipped, Classification::Skipped, false),
    (
        State::Warning,
        State::Skipped,
        Classification::Skipped,
        false,
    ),
    (State::Skipped, State::Pass, Classification::Added, false),
    (State::Skipped, State::Warning, Classification::Added, false),
    (
        State::Skipped,
        State::Absent,
        Classification::UnchangedSkip,
        false,
    ),
];

#[test]
fn classify_agrees_with_the_contract_for_every_state_pair_under_both_policies() {
    // The library's own table, read pair by pair, against the table parsed
    // out of CF-05 — every one of the 36 pairs, skipped and absent included.
    let table = transition_table();
    for strict in [false, true] {
        for from in ALL_STATES {
            for to in ALL_STATES {
                let (classification, regression) = classify(from, to, strict);
                let key = (state_name(from), state_name(to));
                let (expected, policy) = table
                    .get(&key)
                    .unwrap_or_else(|| panic!("CF-05 has no row for {} to {}", key.0, key.1));
                assert_eq!(
                    classification_name(classification),
                    *expected,
                    "{} to {}",
                    key.0,
                    key.1
                );
                let expected_regression = match policy {
                    CountsAsRegression::Yes => true,
                    CountsAsRegression::OnlyWithPolicy => strict,
                    CountsAsRegression::No => false,
                };
                assert_eq!(
                    regression, expected_regression,
                    "{} to {} under warnings_are_failures={strict}",
                    key.0, key.1
                );
            }
        }
    }
}

#[test]
fn skipped_to_failing_is_a_new_failure_and_a_regression() {
    let comparison = compare_pair(
        &[
            (State::Skipped, State::Fail),
            (State::Skipped, State::Error),
        ],
        false,
    );
    for to in [State::Fail, State::Error] {
        let entry = entry(&comparison, State::Skipped, to);
        assert_eq!(entry.from, State::Skipped);
        assert_eq!(entry.to, to);
        assert_eq!(
            entry.classification,
            Classification::AddedFailing,
            "a check that ran for the first time and failed is a new failure"
        );
        assert!(entry.regression, "skipped to {to:?} counts as a regression");
    }
    // Neither was passing in the baseline, so no coverage was lost.
    assert!(comparison.missing.is_empty());
    assert_eq!(comparison.summary.regressed, 2);
    assert_eq!(
        comparison.summary.added, 2,
        "added-failing is counted as added"
    );
    assert_eq!(comparison.verdict, Verdict::Regression);
}

#[test]
fn failing_to_skipped_is_newly_skipped_and_still_a_regression() {
    let comparison = compare_pair(
        &[
            (State::Fail, State::Skipped),
            (State::Error, State::Skipped),
        ],
        false,
    );
    for from in [State::Fail, State::Error] {
        let entry = entry(&comparison, from, State::Skipped);
        assert_eq!(
            entry.classification,
            Classification::SkippedFailing,
            "skipping a failing check hides the failure rather than fixing it"
        );
        assert!(
            entry.regression,
            "{from:?} to skipped counts as a regression"
        );
    }
    // `missing` lists checks that *passed* and are gone; a failing check
    // being skipped is not lost coverage, and it is not resolved either.
    assert!(comparison.missing.is_empty());
    assert_eq!(comparison.summary.missing, 0);
    assert_eq!(comparison.summary.resolved, 0);
    assert_eq!(comparison.summary.regressed, 2);
    assert_eq!(comparison.verdict, Verdict::Regression);
}

#[test]
fn passing_to_skipped_is_lost_coverage_not_a_regression() {
    let comparison = compare_pair(
        &[
            (State::Pass, State::Skipped),
            (State::Warning, State::Skipped),
        ],
        false,
    );
    for from in [State::Pass, State::Warning] {
        let entry = entry(&comparison, from, State::Skipped);
        assert_eq!(entry.classification, Classification::Skipped);
        assert!(!entry.regression);
    }
    assert_eq!(comparison.missing.len(), 2, "both lost checks are listed");
    assert_eq!(comparison.summary.missing, 2);
    assert_eq!(comparison.summary.regressed, 0);
    assert_eq!(comparison.verdict, Verdict::CoverageLost);
}

#[test]
fn the_skip_transition_matrix_summary_matches_its_entries() {
    // Every skip-related row at once, under both policies: the classification
    // and regression flag of each entry, and every summary bucket recomputed
    // from the entries, must agree with the counts the engine reports.
    let transitions: Vec<(State, State)> = SKIP_TRANSITIONS
        .iter()
        .map(|(from, to, _, _)| (*from, *to))
        .collect();
    for strict in [false, true] {
        let comparison = compare_pair(&transitions, strict);
        for (from, to, classification, regression) in SKIP_TRANSITIONS {
            let entry = entry(&comparison, from, to);
            assert_eq!(entry.classification, classification, "{from:?} to {to:?}");
            assert_eq!(
                entry.regression, regression,
                "{from:?} to {to:?}: the policy never changes a skip transition"
            );
        }
        // The anchor plus one entry per transition.
        assert_eq!(comparison.entries.len(), SKIP_TRANSITIONS.len() + 1);

        let count = |wanted: &[Classification]| {
            comparison
                .entries
                .iter()
                .filter(|entry| wanted.contains(&entry.classification))
                .count()
        };
        let summary = comparison.summary;
        assert_eq!(
            summary.regressed,
            comparison.entries.iter().filter(|e| e.regression).count()
        );
        assert_eq!(summary.regressed, 4, "skipped<->failing, both directions");
        assert_eq!(
            summary.added,
            count(&[Classification::Added, Classification::AddedFailing])
        );
        assert_eq!(summary.added, 4);
        assert_eq!(
            summary.unchanged,
            count(&[Classification::Unchanged, Classification::UnchangedSkip])
        );
        assert_eq!(summary.unchanged, 2, "the anchor and skipped to absent");
        assert_eq!(summary.resolved, count(&[Classification::Resolved]));
        assert_eq!(summary.resolved, 0, "no skip transition resolves a failure");
        assert_eq!(
            summary.removed,
            count(&[Classification::Removed, Classification::RemovedFailing])
        );
        assert_eq!(summary.missing, comparison.missing.len());
        assert_eq!(summary.missing, 2, "pass and warning to skipped");
        assert_eq!(comparison.verdict, Verdict::Regression);

        // The serialized document carries the same numbers the struct does.
        let document = json!(comparison);
        assert_eq!(
            as_usize(&document["summary"]["regressed"]),
            summary.regressed
        );
        assert_eq!(as_str(&document["verdict"]), "regression");
    }
}

#[test]
fn a_skip_that_starts_failing_against_an_all_skipped_baseline_is_not_a_regression() {
    // CF-05 section 2: a side with no results cannot establish a regression.
    // A baseline that skipped everything has no results, so the same
    // skipped-to-fail transition is classified identically but not counted.
    let mut baseline = empty_report();
    let mut current = empty_report();
    place(&mut baseline, "skipped-to-fail", State::Skipped);
    place(&mut current, "skipped-to-fail", State::Fail);

    let comparison = compare(&baseline, &current, CompareOptions::default())
        .expect("an all-skipped baseline is comparable");
    assert!(comparison.empty_baseline);
    let entry = &comparison.entries[0];
    assert_eq!(entry.classification, Classification::AddedFailing);
    assert!(
        !entry.regression,
        "an empty baseline suppresses the regression"
    );
    assert_eq!(comparison.summary.regressed, 0);
    assert_eq!(comparison.verdict, Verdict::NoChange);
}

#[test]
fn a_skip_that_stays_skipped_is_unchanged() {
    let comparison = compare_pair(&[(State::Skipped, State::Skipped)], false);
    let entry = entry(&comparison, State::Skipped, State::Skipped);
    assert_eq!(entry.classification, Classification::Unchanged);
    assert!(!entry.regression);
    assert_eq!(comparison.summary.unchanged, 2);
    assert_eq!(comparison.verdict, Verdict::NoChange);
}
