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
