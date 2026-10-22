// Run with: node --test viewer/test/viewer.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const viewerDir = path.join(here, "..");
const V = createRequire(import.meta.url)(path.join(viewerDir, "report.js"));
const read = (name) => readFileSync(path.join(viewerDir, name), "utf8");

const result = (extra = {}) => ({
  testId: "a",
  protocol: 28,
  surface: "xdr",
  status: "pass",
  summary: "",
  durationMs: 0,
  ...extra,
});

function report(overrides = {}) {
  return {
    schemaVersion: 1,
    toolVersion: "0.2.0",
    targetProtocol: 28,
    project: { name: "demo", type: "soroban" },
    status: "pass",
    results: [result({ testId: "t1", summary: "ok", durationMs: 1, fixtureId: "f1" })],
    ...overrides,
  };
}

test("a valid report is accepted and unknown fields are ignored", () => {
  assert.equal(V.validate(report({ somethingNew: { a: 1 } })).ok, true);
});

test("validation reports the first reason for each rule", () => {
  const cases = [
    [null, /JSON object/],
    [report({ schemaVersion: 2 }), /schemaVersion must be 1/],
    [report({ toolVersion: 3 }), /toolVersion/],
    [report({ targetProtocol: "28" }), /targetProtocol/],
    [report({ project: null }), /project must be an object/],
    [report({ status: "great" }), /status must be one of/],
    [report({ results: {} }), /results must be an array/],
    [report({ results: [{}] }), /results\[0\]\.testId/],
    [report({ results: [result({ surface: "evm" })] }), /surface must be one of/],
    [report({ results: [result({ status: "meh" })] }), /status must be one of/],
    [report({ results: [result({ durationMs: -1 })] }), /durationMs/],
  ];
  for (const [input, pattern] of cases) {
    const out = V.validate(input);
    assert.equal(out.ok, false);
    assert.match(out.reason, pattern);
  }
});

test("duplicate result identities are rejected, across results and skipped", () => {
  const dup = report({
    results: [
      result({ surface: "rpc", fixtureId: "x" }),
      result({ testId: "b", surface: "rpc", status: "fail", fixtureId: "x" }),
    ],
  });
  assert.match(V.validate(dup).reason, /repeats the result identity rpc\/x/);

  const skipDup = report({
    results: [result({ surface: "rpc", fixtureId: "x" })],
    skipped: [{ fixtureId: "x", surface: "rpc", reason: "r" }],
  });
  assert.equal(V.validate(skipDup).ok, false);

  const otherSurface = report({
    results: [
      result({ surface: "rpc", fixtureId: "x" }),
      result({ testId: "b", surface: "xdr", fixtureId: "x" }),
    ],
  });
  assert.equal(V.validate(otherSurface).ok, true);
});

test("parseReport rejects text that is not JSON", () => {
  assert.equal(V.parseReport("{nope").ok, false);
  assert.equal(V.parseReport(JSON.stringify(report())).ok, true);
});

test("size limit is 10 MiB and checked from the file size", () => {
  assert.equal(V.checkSize(10 * 1024 * 1024), null);
  assert.match(V.checkSize(10 * 1024 * 1024 + 1), /10 MiB/);
});

test("counts are derived and mismatches are named", () => {
  const r = report({
    results: [result(), result({ testId: "b", status: "fail" })],
    skipped: [{ fixtureId: "s", surface: "rpc", reason: "r" }],
    counts: { total: 5, passed: 1, failed: 1 },
  });
  const derived = V.deriveCounts(r);
  assert.deepEqual(derived, { total: 2, passed: 1, failed: 1, warnings: 0, errors: 0, skipped: 1 });
  assert.deepEqual(V.countMismatches(r, derived), ["total"]);
  assert.deepEqual(V.countMismatches(report(), V.deriveCounts(report())), []);
});

test("headings follow the fixed table and never claim readiness", () => {
  const counts = (total) => ({ total });
  assert.equal(V.heading(report({ status: "pass" }), counts(3)), "All executed checks passed");
  assert.equal(V.heading(report({ status: "pass" }), counts(0)), "No checks ran");
  assert.equal(V.heading(report({ status: "warning" }), counts(1)), "Passed with warnings");
  assert.equal(V.heading(report({ status: "fail" }), counts(1)), "Compatibility failures");
  assert.equal(V.heading(report({ status: "error" }), counts(1)), "Could not complete");
  assert.equal(V.heading(null, null), "Invalid report");
  for (const status of ["pass", "warning", "fail", "error"]) {
    for (const total of [0, 1]) {
      assert.doesNotMatch(V.heading(report({ status }), counts(total)), /Compatible\b|Ready|Supported/);
    }
  }
});

test("the CSP meta tag carries every required directive", () => {
  const match = read("index.html").match(
    /<meta http-equiv="Content-Security-Policy" content="([^"]*)"/,
  );
  assert.ok(match, "CSP meta tag is missing");
  const directives = match[1].split(";").map((d) => d.trim());
  for (const required of [
    "default-src 'none'",
    "script-src 'self'",
    "style-src 'self'",
    "connect-src 'none'",
    "img-src 'self' data:",
    "base-uri 'none'",
    "form-action 'none'",
  ]) {
    assert.ok(directives.includes(required), `missing directive: ${required}`);
  }
});

test("the page loads nothing from outside and has no inline code", () => {
  const html = read("index.html");
  assert.doesNotMatch(html, /https?:\/\//);
  assert.doesNotMatch(html, /<script(?![^>]*\bsrc=)[^>]*>/);
  assert.doesNotMatch(html, /<style[\s>]/);
  assert.doesNotMatch(html, /\son[a-z]+\s*=/i);
  assert.doesNotMatch(html, /style\s*=/i);
  assert.doesNotMatch(read("style.css"), /@import|url\(\s*["']?https?:/);
});

test("scripts never use innerHTML or network APIs and store only the theme", () => {
  for (const name of ["app.js", "report.js"]) {
    const source = read(name);
    assert.doesNotMatch(
      source,
      /innerHTML|outerHTML|insertAdjacentHTML|document\.write|eval\(|new Function/,
      name,
    );
    assert.doesNotMatch(source, /fetch\(|XMLHttpRequest|WebSocket|sendBeacon|EventSource/, name);
    assert.doesNotMatch(source, /sessionStorage|indexedDB|document\.cookie/, name);
  }
  const app = read("app.js");
  const writes = app.match(/localStorage\.setItem\(([^,]+),/g) || [];
  assert.equal(writes.length, 1);
  assert.match(writes[0], /THEME_KEY/);
  assert.equal(V.STORAGE_PREFIX, "stellar-canary-viewer:");
  assert.match(app, /THEME_KEY = V\.STORAGE_PREFIX \+ "theme"/);
});

// ---------------------------------------------------------------------------
// Two-report comparison (CF-05 section 4 through the viewer, CF-08 2.4)
// ---------------------------------------------------------------------------

test("the golden transition table matches the engine classifier row for row", () => {
  assert.equal(V.DIFF_VERSION, 1);
  // One row per line of the CF-05 section 4 table (plus `absent`/`absent`),
  // in the spelling the engine reports: [from, to, warningsAreFailures,
  // classification, regression]. Mirrors `diff.rs::classify`.
  const golden = [
    ["pass", "pass", false, "unchanged", false],
    ["warning", "warning", false, "unchanged", false],
    ["fail", "fail", false, "unchanged", false],
    ["error", "error", false, "unchanged", false],
    ["skipped", "skipped", false, "unchanged", false],
    ["absent", "absent", false, "unchanged", false],
    ["pass", "warning", false, "degraded", false],
    ["pass", "warning", true, "degraded", true],
    ["pass", "fail", false, "regressed", true],
    ["warning", "fail", false, "regressed", true],
    ["pass", "error", false, "regressed", true],
    ["warning", "error", false, "regressed", true],
    ["fail", "error", false, "degraded", true],
    ["error", "fail", false, "improved", false],
    ["fail", "pass", false, "resolved", false],
    ["fail", "warning", false, "resolved", false],
    ["error", "pass", false, "resolved", false],
    ["error", "warning", false, "resolved", false],
    ["warning", "pass", false, "improved", false],
    ["absent", "pass", false, "added", false],
    ["skipped", "pass", false, "added", false],
    ["skipped", "warning", false, "added", false],
    ["absent", "warning", false, "added-failing", true],
    ["absent", "fail", false, "added-failing", true],
    ["absent", "error", false, "added-failing", true],
    ["skipped", "fail", false, "added-failing", true],
    ["skipped", "error", false, "added-failing", true],
    ["pass", "absent", false, "removed", false],
    ["warning", "absent", false, "removed", false],
    ["fail", "absent", false, "removed-failing", false],
    ["error", "absent", false, "removed-failing", false],
    ["pass", "skipped", false, "skipped", false],
    ["warning", "skipped", false, "skipped", false],
    ["fail", "skipped", false, "skipped-failing", true],
    ["error", "skipped", false, "skipped-failing", true],
    ["skipped", "absent", false, "unchanged-skip", false],
    ["absent", "skipped", false, "unchanged-skip", false],
  ];
  for (const [from, to, warningsAreFailures, classification, regression] of golden) {
    const classified = V.classifyTransition(from, to, warningsAreFailures);
    assert.deepEqual(
      classified,
      { classification, regression },
      `transition ${from} -> ${to} with warningsAreFailures=${warningsAreFailures}`,
    );
  }
});

test("a report pair about different target protocols is refused, never compared", () => {
  const outcome = V.compareReports(
    report({ targetProtocol: 28 }),
    report({ targetProtocol: 29 }),
  );
  assert.equal(outcome.ok, false);
  assert.match(outcome.reason, /target protocol 28/);
  assert.match(outcome.reason, /target protocol 29/);
  assert.match(outcome.reason, /not comparable/);
});

test("a side that is not a validated schemaVersion 1 report is refused", () => {
  const wrongVersion = V.compareReports(report(), report({ schemaVersion: 2 }));
  assert.equal(wrongVersion.ok, false);
  assert.match(wrongVersion.reason, /schemaVersion 1/);

  const notObject = V.compareReports("not a report", report());
  assert.equal(notObject.ok, false);
  assert.match(notObject.reason, /baseline/);

  const currentMissing = V.compareReports(report(), null);
  assert.equal(currentMissing.ok, false);
  assert.match(currentMissing.reason, /current/);
});

test("duplicate identities inside either side are refused before any comparison", () => {
  const dupBaseline = report({
    results: [
      result({ surface: "rpc", fixtureId: "x", status: "pass" }),
      result({ testId: "b", surface: "rpc", fixtureId: "x", status: "fail" }),
    ],
  });
  const outcome = V.compareReports(dupBaseline, report());
  assert.equal(outcome.ok, false);
  assert.match(outcome.reason, /baseline/);
  assert.match(outcome.reason, /repeats the result identity rpc\/x/);
});

function compared(baselineResults, currentResults, baselineOverrides = {}, currentOverrides = {}) {
  const baseline = report({
    status: "pass",
    results: baselineResults.map((r) => result(r)),
    ...baselineOverrides,
  });
  const current = report({
    status: "fail",
    results: currentResults.map((r) => result(r)),
    ...currentOverrides,
  });
  const outcome = V.compareReports(baseline, current);
  assert.equal(outcome.ok, true, JSON.stringify(outcome.reason));
  return outcome.comparison;
}

test("a full comparison mirrors the engine: entries, missing, summary, verdict", () => {
  const comparison = compared(
    [
      { testId: "rpc-kept", surface: "rpc", fixtureId: "rpc-kept", status: "pass" },
      { testId: "rpc-broke", surface: "rpc", fixtureId: "rpc-broke", status: "pass" },
      { testId: "rpc-fixed", surface: "rpc", fixtureId: "rpc-fixed", status: "fail" },
      { testId: "rpc-lost", surface: "rpc", fixtureId: "rpc-lost", status: "pass" },
      { testId: "a-lot", surface: "xdr", fixtureId: "a-lot", status: "warning" },
    ],
    [
      { testId: "rpc-kept", surface: "rpc", fixtureId: "rpc-kept", status: "pass" },
      { testId: "rpc-broke", surface: "rpc", fixtureId: "rpc-broke", status: "fail" },
      { testId: "rpc-fixed", surface: "rpc", fixtureId: "rpc-fixed", status: "pass" },
      { testId: "a-lot", surface: "xdr", fixtureId: "a-lot", status: "pass" },
    ],
    { status: "pass" },
    { status: "fail" },
  );

  assert.equal(comparison.diffVersion, 1);
  assert.equal(comparison.comparable, true);
  assert.deepEqual(comparison.baseline, {
    toolVersion: "0.2.0",
    targetProtocol: 28,
    status: "pass",
  });
  assert.deepEqual(comparison.current, {
    toolVersion: "0.2.0",
    targetProtocol: 28,
    status: "fail",
  });

  // Entries sorted by (surface, id) in UTF-8 byte order.
  assert.deepEqual(
    comparison.entries.map((e) => [e.surface, e.id]),
    [
      ["rpc", "rpc-broke"],
      ["rpc", "rpc-fixed"],
      ["rpc", "rpc-kept"],
      ["rpc", "rpc-lost"],
      ["xdr", "a-lot"],
    ],
  );
  const byId = Object.fromEntries(
    comparison.entries.map((e) => [e.id, e]),
  );
  assert.deepEqual(byId["rpc-broke"], {
    surface: "rpc",
    id: "rpc-broke",
    from: "pass",
    to: "fail",
    classification: "regressed",
    regression: true,
    sourceChanged: null,
  });
  assert.equal(byId["rpc-fixed"].classification, "resolved");
  assert.equal(byId["rpc-kept"].classification, "unchanged");
  assert.equal(byId["a-lot"].classification, "improved");
  assert.equal(byId["rpc-kept"].regression, false);

  // The lost check: passed in the baseline, absent in the current run.
  assert.equal(byId["rpc-lost"].classification, "removed");
  assert.equal(byId["rpc-lost"].regression, false);
  assert.deepEqual(comparison.missing, [{ surface: "rpc", id: "rpc-lost" }]);
  assert.deepEqual(comparison.summary, {
    regressed: 1,
    resolved: 1,
    added: 0,
    removed: 1,
    unchanged: 1,
    missing: 1,
  });
  // Regression wins the first-match-wins verdict even with coverage lost.
  assert.equal(comparison.verdict, "regression");
  assert.equal(comparison.observedProtocolChanged, false);
  assert.equal(comparison.emptyBaseline, false);
  assert.equal(comparison.emptyCurrent, false);
});

test("shuffling either input gives the same comparison bytes", () => {
  const baselineResults = [
    { testId: "rpc-b", surface: "rpc", fixtureId: "rpc-b", status: "pass" },
    { testId: "rpc-a", surface: "rpc", fixtureId: "rpc-a", status: "pass" },
  ];
  const forward = compared(baselineResults, [], { status: "pass" }, { status: "pass" });
  const shuffled = compared(
    [baselineResults[1], baselineResults[0]],
    [],
    { status: "pass" },
    { status: "pass" },
  );
  assert.deepEqual(JSON.parse(JSON.stringify(shuffled)), JSON.parse(JSON.stringify(forward)));
  // Two removed checks carry the coverage-lost verdict on an empty current
  // side, without any being called a regression.
  assert.deepEqual(forward.summary, {
    regressed: 0,
    resolved: 0,
    added: 0,
    removed: 2,
    unchanged: 0,
    missing: 2,
  });
  assert.equal(forward.verdict, "coverage-lost");
});

test("coverage lost is the verdict when no regression and a check disappeared", () => {
  const comparison = compared(
    [{ testId: "rpc-gone", surface: "rpc", fixtureId: "rpc-gone", status: "pass" }],
    [],
    { status: "pass" },
    { status: "fail" },
  );
  assert.deepEqual(comparison.summary, {
    regressed: 0,
    resolved: 0,
    added: 0,
    removed: 1,
    unchanged: 0,
    missing: 1,
  });
  assert.equal(comparison.verdict, "coverage-lost");
  assert.equal(comparison.emptyCurrent, true);
});

test("an empty side can never establish a regression (CF-05 section 2)", () => {
  // Baseline ran nothing at all; the current run adds a failing check.
  const withEmptyBaseline = compared(
    [],
    [{ testId: "rpc-new", surface: "rpc", fixtureId: "rpc-new", status: "fail" }],
    { status: "pass" },
  );
  assert.equal(withEmptyBaseline.emptyBaseline, true);
  assert.equal(withEmptyBaseline.entries[0].classification, "added-failing");
  assert.equal(withEmptyBaseline.entries[0].regression, false);
  assert.deepEqual(withEmptyBaseline.summary, {
    regressed: 0,
    resolved: 0,
    added: 1,
    removed: 0,
    unchanged: 0,
    missing: 0,
  });
  assert.equal(withEmptyBaseline.verdict, "no-change");

  // Same equality protection for an empty current run: a removed failing
  // check is not a fix and not a regression if the other side was empty.
  const withEmptyCurrent = compared(
    [{ testId: "rpc-old", surface: "rpc", fixtureId: "rpc-old", status: "fail" }],
    [],
  );
  assert.equal(withEmptyCurrent.emptyCurrent, true);
  assert.equal(withEmptyCurrent.entries[0].regression, false);
});

test("fixture pack and observed protocol differences are recorded, never gating", () => {
  const bothPacks = compared(
    [],
    [],
    { status: "pass", fixturePack: { digest: "sha256:" + "aa".repeat(32) } },
    { status: "pass", fixturePack: { digest: "sha256:" + "bb".repeat(32) } },
  );
  assert.equal(bothPacks.fixturePackChanged, true);
  assert.equal(bothPacks.verdict, "no-change");

  const samePacks = compared(
    [],
    [],
    { status: "pass", fixturePack: { digest: "sha256:" + "aa".repeat(32) } },
    { status: "pass", fixturePack: { digest: "sha256:" + "aa".repeat(32) } },
  );
  assert.equal(samePacks.fixturePackChanged, false);

  const noPack = compared([], []);
  assert.equal(noPack.fixturePackChanged, null);

  const protocolJump = compared(
    [],
    [],
    { status: "pass", network: { name: "testnet", observedProtocol: 28 } },
    { status: "pass", network: { name: "testnet", observedProtocol: 29 } },
  );
  assert.equal(protocolJump.observedProtocolChanged, true);
});

test("cached results keep their transitions and say when the source changed", () => {
  const comparison = compared(
    [{ testId: "a", surface: "rpc", fixtureId: "a", status: "pass", source: "live" }],
    [{ testId: "a", surface: "rpc", fixtureId: "a", status: "pass", source: "cache" }],
  );
  assert.equal(comparison.entries[0].sourceChanged, true);
  assert.equal(comparison.entries[0].classification, "unchanged");

  const cacheBoth = compared(
    [{ testId: "a", surface: "rpc", fixtureId: "a", status: "pass", source: "cache" }],
    [{ testId: "a", surface: "rpc", fixtureId: "a", status: "pass", source: "cache" }],
  );
  assert.equal(cacheBoth.entries[0].sourceChanged, false);

  const noCache = compared(
    [{ testId: "a", surface: "rpc", fixtureId: "a", status: "pass", source: "live" }],
    [{ testId: "a", surface: "rpc", fixtureId: "a", status: "pass", source: "live" }],
  );
  assert.equal(noCache.entries[0].sourceChanged, null);
});

test("verdict headings are fixed and incomparable never gets one", () => {
  assert.equal(V.verdictHeading("regression"), "Regression");
  assert.equal(V.verdictHeading("coverage-lost"), "Coverage lost");
  assert.equal(V.verdictHeading("improved"), "Improved");
  assert.equal(V.verdictHeading("no-change"), "No change");
  assert.equal(V.verdictHeading("incomparable"), "Not comparable");
  for (const verdict of ["regression", "coverage-lost", "improved", "no-change"]) {
    assert.doesNotMatch(V.verdictHeading(verdict), /Ready|Compatible|Supported/);
  }
});

test("the comparison markup exists, is wired, and every element app.js references is present", () => {
  const html = read("index.html");
  for (const id of [
    "compare",
    "compare-decision",
    "baseline-drop",
    "baseline-file",
    "current-drop",
    "current-file",
    "compare-status",
    "compare-result",
    "compare-ineligible",
    "compare-verdict",
    "compare-summary",
    "compare-table",
    "compare-body",
  ]) {
    assert.ok(html.includes(`id="${id}"`), `missing #${id} in index.html`);
  }

  // The refusal banner stays an alert region, and the verdict area is a
  // heading so the verdict is announced as a section title.
  assert.match(html, /id="compare-ineligible"[^>]*role="alert"/);
  assert.match(html, /id="compare-result"[^>]*aria-live="polite"/);
  assert.match(html, /<h3 id="compare-verdict"/);

  // Both prompts name the direction, so nobody compares the wrong way
  // round.
  assert.match(html, /Baseline report \(earlier run\)/);
  assert.match(html, /Current report \(later run\)/);

  // Every el("…") reference in app.js is an id that exists on the page.
  const app = read("app.js");
  const referenced = new Set();
  for (const match of app.matchAll(/el\("([A-Za-z0-9-]+)"\)/g)) {
    referenced.add(match[1]);
  }
  for (const id of referenced) {
    assert.ok(html.includes(`id="${id}"`), `app.js references #${id}, index.html has none`);
  }

  // The comparison table renders per-check rows, not innerHTML messages.
  assert.match(read("style.css"), /#compare-result\[data-verdict="regression"\]/);
});
