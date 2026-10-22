// Malicious-report and no-network regression tests for the static viewer
// (E-089 / CF-08-H). Run with:
//   node --test viewer/test/security.test.mjs
//
// The corpus: script tags, HTML, event-handler attributes, tracking pixels
// and javascript: URLs in every string field a report carries, oversized
// strings, deeply nested JSON, duplicate identities, unknown enums and a
// wrong schemaVersion. The outcome demanded by CF-08: hostile text is
// either refused with the CF-01 reason or displayed as inert text — never
// turned into markup, attributes or element creation — and no network
// request is ever attempted (the page runs with connect-src 'none').
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { createStubDom, loadReport } from "./stub-dom.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const viewerDir = path.join(here, "..");
const read = (name) => readFileSync(path.join(viewerDir, name), "utf8");
const V = createRequire(import.meta.url)(path.join(viewerDir, "report.js"));

/* A report in the accepted schema whose every string field carries an
 * attack payload. Validation must accept it (all payloads are strings),
 * so the only defence left is escaping — which the DOM tests exercise. */
function maliciousReport(overrides = {}) {
  return {
    schemaVersion: 1,
    toolVersion: "0.2.0<script>alert('toolVersion')</script>",
    targetProtocol: 28,
    project: {
      name: "<img src=https://tracker.example/pixel?x=1 onerror=alert(1)>",
      type: "<b>soroban</b>",
    },
    status: "fail",
    results: [
      {
        testId: '"><script>alert("testId")</script>',
        protocol: 28,
        surface: "xdr",
        status: "fail",
        summary: 'x" onclick=alert(2)&gt;<iframe src=javascript:alert(3)></iframe>',
        durationMs: 0,
        fixtureId: "f<script>alert('fixtureId')</script>",
        details: "<svg><script>alert('details')</script></svg><img src=x onerror=alert(4)>",
      },
    ],
    skipped: [
      {
        fixtureId: "<script>alert('skipped')</script>",
        surface: "rpc",
        reason: "<script>alert('skipped.reason')</script>",
      },
    ],
    network: {
      name: '<iframe src="javascript:alert(5)"></iframe>',
      observedProtocol: 20,
    },
    ...overrides,
  };
}

function collectNetworks() {
  const called = [];
  globalThis.fetch = (...args) => called.push(["fetch", ...args]);
  return called;
}

function resetNetworks() {
  delete globalThis.fetch;
}

/* --- validation layer: attack payloads inside the accepted schema --- */

test("a hostile report is valid per CF-01 and parsed without side effects", () => {
  const parsed = V.parseReport(JSON.stringify(maliciousReport()));
  assert.equal(parsed.ok, true, JSON.stringify(parsed.reason ?? null));
});

test("a 1 MiB string inside the accepted schema is carried through as inert text", () => {
  const big = maliciousReport();
  big.results[0].summary = "A".repeat(1024 * 1024);
  const parsed = V.parseReport(JSON.stringify(big));
  assert.equal(parsed.ok, true);
});

test("structural attacks are refused with the CF-01 reason, never thrown", () => {
  const deepPayload = Array.from({ length: 400 }).reduce((acc) => [acc], "seed");
  const deep = maliciousReport();
  deep.results[0].details = deepPayload;

  const dup = maliciousReport({
    results: [
      { testId: "a", protocol: 28, surface: "rpc", status: "pass", summary: "", durationMs: 0, fixtureId: "x" },
      { testId: "b", protocol: 28, surface: "rpc", status: "fail", summary: "", durationMs: 0, fixtureId: "x" },
    ],
  });
  const badEnum = maliciousReport();
  badEnum.results[0].surface = "evm";
  const badStatus = maliciousReport({ status: "excellent" });
  const wrongVersion = { ...maliciousReport(), schemaVersion: 2 };

  const cases = [
    ["{unterminated", /not valid JSON/],
    [JSON.stringify(wrongVersion), /schemaVersion must be 1/],
    [JSON.stringify(dup), /repeats the result identity/],
    [JSON.stringify(badEnum), /surface must be one of/],
    [JSON.stringify(badStatus), /status must be one of/],
    [JSON.stringify(deep), /details must be a string/],
  ];
  for (const [text, pattern] of cases) {
    const outcome = V.parseReport(text);
    assert.equal(outcome.ok, false, "expected refusal for " + pattern);
    assert.match(outcome.reason, pattern);
  }
});

/* --- rendering layer: the page must not turn payloads into markup --- */

test("rendering the malicious report keeps every payload inert in text nodes", async () => {
  const called = collectNetworks();
  try {
    const stub = createStubDom();
    stub.runApp();
    const outcome = await loadReport(stub, JSON.stringify(maliciousReport()));

    // The file went in unblessed: text in, nothing read twice.
    assert.equal(outcome.textCalls, 1);

    // The report is displayed (fail status), not swallowed.
    assert.equal(stub.byId("result").hidden, false);

    // No HTML sink was ever touched (the stub traps innerHTML, outerHTML,
    // insertAdjacentHTML and document.write by throwing on contact), and
    // createElement never produced anything beyond the allowlist {dt, dd,
    // li, span, details, summary, p} — no img/link/script/iframe/svg/a,
    // so no tracking pixel or remote script can enter the DOM.
    assert.deepEqual(
      [...stub.createdTags].sort(),
      ["dd", "details", "dt", "li", "p", "span", "summary"],
    );

    // No attribute is ever set to a URL, an event handler or a tag string:
    // only class/aria/data/tabindex states are allowed.
    for (const { name, value } of stub.attributes) {
      assert.doesNotMatch(name, /^on/i, "event handler attribute written: " + name);
      assert.ok(
        !["src", "href", "rel", "style", "integrity", "nonce", "srcset", "action"].includes(name.toLowerCase()),
        "attribute that gets executed or fetched set: " + name,
      );
      assert.doesNotMatch(value, /https?:|javascript:/i, "URL smuggled into attribute " + name);
      if (name !== "aria-label") {
        // aria-label echoes hostile fixture ids as inert text by design.
        assert.doesNotMatch(value, /<\s*[a-z]/i, "markup smuggled into attribute " + name);
      }
    }

    // The payloads stay verbatim in the text of the page — inert characters.
    const visible = [
      stub.byId("heading").textContent,
      stub.byId("facts").textContent,
      ["ready", "attention", "incomplete"]
        .map((key) => stub.byId(key + "-list").textContent)
        .join("\n"),
      stub.byId("skipped-list").textContent,
      stub.byId("protocol-mismatch").textContent,
      stub.byId("report-source-code").textContent,
    ].join("\n");
    assert.ok(/<script>alert\('fixtureId'\)<\/script>/.test(visible), "fixtureId payload is present as text");
    assert.ok(/tracker\.example/.test(visible), "the tracking-pixel URL is present as text only");

    // The mismatch banner (target 28 vs observed 20) is announced as text.
    assert.match(stub.byId("protocol-mismatch").textContent, /Protocol Mismatch Warning/);

    // Zero network requests were dispatched end to end.
    assert.deepEqual(called, []);
  } finally {
    resetNetworks();
  }
});

test("the malicious file text lands uninfluenced in the source dialog", async () => {
  const stub = createStubDom();
  stub.runApp();
  const text = JSON.stringify(maliciousReport());
  await loadReport(stub, text);

  stub.byId("open-source").dispatch("click", {});
  assert.equal(stub.modalOps[stub.modalOps.length - 1], "show");
  assert.equal(stub.byId("report-source-code").textContent, text);
  assert.match(
    stub.byId("report-source-note").textContent,
    /exactly as received/,
  );

  stub.byId("close-source").dispatch("click", {});
  assert.equal(stub.modalOps[stub.modalOps.length - 1], "close");
});

test("an over-sized file is refused before it is read", async () => {
  const stub = createStubDom();
  stub.runApp();
  const out = await loadReport(stub, JSON.stringify(maliciousReport()), 10 * 1024 * 1024 + 1);
  assert.equal(out.textCalls, 0, "the 10 MiB limit is checked from the size without reading");
  assert.equal(stub.byId("result").hidden, false, "the refusal is displayed");
  assert.match(stub.byId("invalid-reason").textContent, /10 MiB/);
  assert.equal(stub.byId("open-source").hidden, true, "no unread text is ever offered");
  assert.equal(stub.byId("findings").hidden, true);
});

test("files without remote references of any kind", () => {
  const html = read("index.html");
  const css = read("style.css");
  const js = read("app.js");
  const lib = read("report.js");

  // The CSP carries connect-src 'none', so even a bug cannot phone home.
  assert.match(html, /content="[^"]*connect-src 'none'[^"]*"/);

  // No absolute URLs anywhere in the shipped assets.
  for (const [name, source] of [["index.html", html], ["style.css", css], ["app.js", js], ["report.js", lib]]) {
    assert.doesNotMatch(source, /https?:\/\//, name + " contains an external URL");
  }

  // Every script and stylesheet reference stays relative, so the copied
  // bundle works from any base path (E-090 requirement).
  for (const m of html.matchAll(/(?:src|href)="([^"]+)"/g)) {
    const url = m[1];
    assert.doesNotMatch(url, /^([a-z]+:|\/\/|\/)/i, "non-relative reference in index.html: " + url);
  }

  // No runtime network API in the shipped scripts.
  for (const [name, source] of [["app.js", js], ["report.js", lib]]) {
    assert.doesNotMatch(
      source,
      /\b(fetch\s*\(|XMLHttpRequest|WebSocket|sendBeacon|EventSource|import\s*\(|navigator\.sendBeacon)/,
      name + " contains a network API",
    );
  }
});

test("denied localStorage falls back to the system theme instead of crashing", async () => {
  const stub = createStubDom({ allowStorage: false, systemDark: true });
  stub.runApp();

  // The denied read was attempted exactly once and swallowed.
  assert.equal(stub.storageAttempts.reads, 1);

  // No forced theme: the page follows the OS and reports it via state.
  assert.equal(stub.documentElement.hasAttribute("data-theme"), false);
  assert.equal(stub.byId("theme").getAttribute("aria-pressed"), "true");

  // The toggle still works; only the write is lost.
  stub.byId("theme").dispatch("click", {});
  assert.equal(stub.documentElement.getAttribute("data-theme"), "light");
  assert.equal(stub.byId("theme").getAttribute("aria-pressed"), "false");
  assert.ok(stub.storageAttempts.writes > 0, "the write was attempted and swallowed");

  // And the whole page still renders a report end to end with storage denied.
  const stub2 = createStubDom({ allowStorage: false });
  stub2.runApp();
  const out = await loadReport(stub2, JSON.stringify(maliciousReport()));
  assert.equal(out.textCalls, 1);
  assert.equal(stub2.byId("result").hidden, false);
});
