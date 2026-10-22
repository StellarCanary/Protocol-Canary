// Automated accessibility checks for the static viewer (E-087 / CF-08-E)
// plus the theme contract of E-088. Run with:
//   node --test viewer/test/a11y.test.mjs
//
// Two layers:
// - static checks on the shipped markup and CSS (roles, labels, the live
//   region, focus-visible rules, the native modal dialog),
// - behavioural checks that drive app.js inside the stub DOM: the ARIA
//   tabs pattern with its keyboard support, the search live region, the
//   theme toggle state, and the WCAG AA contrast of every colour pair.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createStubDom, loadReport } from "./stub-dom.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const viewerDir = path.join(here, "..");
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
    results: [
      result({ testId: "t1", surface: "xdr", status: "pass", summary: "xdr ok", fixtureId: "f1" }),
      result({ testId: "t2", surface: "rpc", status: "fail", summary: "rpc broken", fixtureId: "f2", details: "explains the failure" }),
      result({ testId: "t3", surface: "soroban", status: "warning", summary: "soroban warn", fixtureId: "f3" }),
      result({ testId: "t4", surface: "rpc", status: "error", summary: "rpc stopped", fixtureId: "f4" }),
    ],
    skipped: [
      { fixtureId: "s1", surface: "xdr", reason: "not applicable" },
    ],
    ...overrides,
  };
}

test("index.html carries the tablist pattern with a tabpanel and selected states", () => {
  const html = read("index.html");
  assert.match(html, /role="tablist"/);
  assert.match(html, /role="tabpanel"/);
  const tabs = [...html.matchAll(/role="tab"/g)];
  assert.ok(tabs.length >= 4, "the four surface tabs must have role=tab");
  const selected = [...html.matchAll(/role="tab"[^>]*aria-selected="true"/g)];
  assert.equal(selected.length, 1, "exactly one tab is selected on load");
});

test("every ARIA tab is a real button so Enter and Space activate it", () => {
  const html = read("index.html");
  const tabIds = [...html.matchAll(/id="tab-([a-z]+)"[^>]*role="tab"/g)].map((m) => m[1]);
  assert.deepEqual(tabIds.sort(), ["all", "rpc", "soroban", "xdr"]);
  for (const id of tabIds) {
    assert.match(
      html,
      new RegExp(`<button id="tab-${id}"[^>]*role="tab"`),
      id + " tab must be a <button>",
    );
    assert.match(
      html,
      new RegExp(`<button id="tab-${id}"[^>]*aria-controls="panel-findings"`),
      id + " tab must reference the tabpanel",
    );
  }
});

test("the search field is labelled and announces its live status politely", () => {
  const html = read("index.html");
  assert.match(html, /<label for="search">Search findings<\/label>/);
  assert.match(html, /<input id="search" type="search"[^>]*aria-controls="panel-findings"/);
  assert.match(html, /id="search-status" aria-live="polite"/);
});

test("the theme toggle and the modal dialog expose states to assistive tech", () => {
  const html = read("index.html");
  assert.match(html, /<button id="theme" type="button" aria-pressed="false">/);
  assert.match(html, /<dialog id="report-source" aria-labelledby="report-source-title">/);
  assert.match(html, /<button id="close-source"/, "the dialog has a visible close button");
  assert.match(html, /<pre tabindex="0">/, "the JSON preview is keyboard reachable");
});

test("focus is always drawn with a visible 3px accent ring", () => {
  const css = read("style.css");
  const rule = css.match(
    /button:focus-visible,\s*input:focus-visible,\s*summary:focus-visible,\s*a:focus-visible,\s*\[tabindex\]:focus-visible\s*\{[^}]*\}/,
  );
  assert.ok(rule, "focus-visible rule must cover button, input, summary, a and [tabindex]");
  assert.match(rule[0], /outline:\s*3px solid var\(--accent\)/);
  // Both palettes meet 3:1 for --accent against --bg and --surface, so the
  // same ring is visible in light and dark; the contrast test pins the hues.
});

/* --- behavioural checks: app.js inside the stub DOM --- */

function dom() {
  const stub = createStubDom();
  stub.runApp();
  return stub;
}

async function loaded(stub, overrides) {
  const out = await loadReport(stub, JSON.stringify(report(overrides)));
  assert.equal(out.textCalls, 1, "the file is read exactly once");
  return out;
}

/* The cards currently on show, summed over the three readiness groups. */
function visibleCards(stub) {
  return ["ready", "attention", "incomplete"].reduce(
    (n, key) => n + stub.byId(key + "-list").children.length,
    0,
  );
}

test("tabs start selected with a roving tabindex and label the panel", () => {
  const stub = createStubDom();
  stub.runApp();
  assert.equal(stub.byId("tab-all").getAttribute("aria-selected"), "true");
  assert.equal(stub.byId("tab-xdr").getAttribute("aria-selected"), "false");
  for (const key of ["all", "rpc", "xdr", "soroban"]) {
    const selected = key === "all";
    assert.equal(stub.byId("tab-" + key).getAttribute("tabindex"), selected ? "0" : "-1");
  }
  assert.equal(stub.byId("panel-findings").getAttribute("aria-labelledby"), "tab-all");
  assert.equal(stub.byId("theme").getAttribute("aria-pressed"), "false");
});

test("arrow keys, Home and End move the tabs pattern without clashing with page Tab order", () => {
  const stub = dom();

  stub.byId("tabs").dispatch("keydown", { key: "ArrowRight", preventDefault() {} });
  assert.equal(stub.byId("tab-rpc").getAttribute("aria-selected"), "true");
  assert.equal(stub.byId("tab-all").getAttribute("aria-selected"), "false");
  assert.equal(stub.byId("tab-rpc").getAttribute("tabindex"), "0");
  assert.equal(stub.byId("tab-all").getAttribute("tabindex"), "-1");
  assert.equal(stub.byId("panel-findings").getAttribute("aria-labelledby"), "tab-rpc");
  assert.deepEqual(stub.focusLog.slice(-1), ["tab-rpc"]);

  stub.byId("tabs").dispatch("keydown", { key: "ArrowLeft", preventDefault() {} });
  assert.equal(stub.byId("tab-all").getAttribute("aria-selected"), "true");

  stub.byId("tabs").dispatch("keydown", { key: "End", preventDefault() {} });
  assert.equal(stub.byId("tab-soroban").getAttribute("aria-selected"), "true");
  stub.byId("tabs").dispatch("keydown", { key: "Home", preventDefault() {} });
  assert.equal(stub.byId("tab-all").getAttribute("aria-selected"), "true");

  // Plain Tab keyboard nav stays untouched: the tablist keydown handler
  // must not swallow anything but the four roving keys.
  stub.byId("tabs").dispatch("keydown", { key: "Tab" });
  assert.equal(stub.byId("tab-all").getAttribute("aria-selected"), "true");
});

test("clicking a tab switches the displayed surface instantly", async () => {
  const stub = dom();
  await loaded(stub, {});

  assert.equal(stub.byId("search-status").textContent, "Showing all 4 results. 1 of 1 skipped fixtures shown.");
  assert.equal(visibleCards(stub), 4);

  stub.byId("tab-rpc").dispatch("click", {});
  assert.equal(stub.byId("tab-rpc").getAttribute("aria-selected"), "true");
  assert.equal(stub.byId("tab-all").getAttribute("aria-selected"), "false");
  assert.equal(stub.byId("panel-findings").getAttribute("aria-labelledby"), "tab-rpc");
  // Of the two rpc results, the failure lands in Attention Required and
  // the error in Incomplete; Ready has nothing left to show and hides.
  assert.equal(stub.byId("ready-list").children.length, 0);
  assert.equal(stub.byId("group-ready").hidden, true);
  const attention = stub.byId("attention-list").children;
  assert.equal(attention.length, 1);
  assert.equal(attention[0].attributes.get("class"), "finding");
  const incomplete = stub.byId("incomplete-list").children;
  assert.equal(incomplete.length, 1);
  assert.equal(incomplete[0].textContent, "errorrpcf4rpc stopped0 ms");
  assert.equal(stub.byId("group-incomplete").hidden, false);
});

test("the search live region reports filtered counts and empty states politely", async () => {
  const stub = dom();
  await loaded(stub, {});

  stub.byId("search").value = "rpc";
  stub.byId("search").dispatch("input", {});
  assert.equal(stub.byId("search-status").textContent, "2 of 4 results match \"rpc\". 0 of 1 skipped fixtures shown.");
  assert.equal(visibleCards(stub), 2);

  stub.byId("search").value = "nothing-matches-this";
  stub.byId("search").dispatch("input", {});
  assert.equal(
    stub.byId("search-status").textContent,
    "No results match \"nothing-matches-this\". 0 of 1 skipped fixtures shown.",
  );
  assert.equal(visibleCards(stub), 0);
  // No match hides every group whole: the empty-state notice is the only
  // thing left in the panel, and the layout does not keep empty headings.
  for (const key of ["ready", "attention", "incomplete"]) {
    assert.equal(stub.byId("group-" + key).hidden, true, "group-" + key);
  }
  assert.equal(
    stub.byId("findings-empty").textContent,
    "No results match \"nothing-matches-this\".",
  );
  assert.equal(stub.byId("findings-empty").hidden, false);

  stub.byId("search").value = "";
  stub.byId("search").dispatch("input", {});
  assert.equal(stub.byId("search-status").textContent, "Showing all 4 results. 1 of 1 skipped fixtures shown.");
  assert.equal(stub.byId("findings-empty").hidden, true);
});

test("loading a new file resets the previous tab and query state", async () => {
  const stub = dom();
  await loaded(stub, {});

  stub.byId("tab-xdr").dispatch("click", {});
  stub.byId("search").value = "nope";
  stub.byId("search").dispatch("input", {});
  await loaded(stub, {});

  assert.equal(stub.byId("tab-all").getAttribute("aria-selected"), "true");
  assert.equal(stub.byId("search").value, "");
  assert.equal(stub.byId("search-status").textContent, "Showing all 4 results. 1 of 1 skipped fixtures shown.");
});

test("findings cards expose surface, id, status, timing and a collapsible details toggle", async () => {
  const stub = dom();
  await loaded(stub, {});

  assert.equal(visibleCards(stub), 4);
  const failing = stub.byId("attention-list").children[0];
  assert.equal(failing.textContent, "failrpcf2rpc broken0 msDetailsexplains the failure");
  const spansByName = {};
  for (const child of failing.children) {
    const cls = child.attributes.get("class");
    spansByName[cls] = child.textContent;
  }
  assert.equal(spansByName.meta, "rpc");
  assert.equal(spansByName.identity, "f2");
  assert.equal(spansByName.summary, "rpc broken");
  assert.equal(spansByName.status, "fail");
  assert.equal(spansByName.timing, "0 ms");
  assert.equal(
    [...failing.children].find((c) => c.attributes.get("class") === "status").attributes.get("data-status"),
    "fail",
  );

  const details = failing.children.find((c) => c.tagName === "DETAILS");
  assert.ok(details, "a result with details gets a collapsible block");
  assert.equal(details.children[0].textContent, "Details");
  assert.equal(details.children[0].attributes.get("aria-label"), "Details of f2");
  assert.equal(details.children[1].textContent, "explains the failure");

  const plain = stub.byId("ready-list").children[0];
  assert.ok(!plain.children.some((c) => c.tagName === "DETAILS"), "no toggle without details");
  assert.ok(!plain.children.some((c) => c.tagName === "SUMMARY"));

  const skippedRows = stub.byId("skipped-list").children;
  assert.equal(skippedRows.length, 1);
  assert.equal(skippedRows[0].textContent, "skippedxdrs1not applicable");
  assert.ok(!stub.byId("skipped-section").hidden);
});

test("results are grouped under readiness headings with live counts and formatted timings", async () => {
  const stub = dom();
  await loaded(stub, {
    results: [
      result({ testId: "t1", surface: "xdr", status: "pass", fixtureId: "f1", durationMs: 12 }),
      result({ testId: "t2", surface: "rpc", status: "warning", fixtureId: "f2", durationMs: 1500 }),
      result({ testId: "t3", surface: "rpc", status: "fail", fixtureId: "f3", durationMs: 999 }),
      result({ testId: "t4", surface: "rpc", status: "error", fixtureId: "f4" }),
      result({ testId: "t5", surface: "xdr", status: "skipped", fixtureId: "f5" }),
    ],
    skipped: [],
  });

  // pass -> Ready; warning and fail -> Attention Required; error and
  // skipped -> Incomplete. Each heading carries its count.
  assert.equal(stub.byId("ready-heading").textContent, "Ready (1)");
  assert.equal(stub.byId("attention-heading").textContent, "Attention Required (2)");
  assert.equal(stub.byId("incomplete-heading").textContent, "Incomplete (2)");
  for (const key of ["ready", "attention", "incomplete"]) {
    assert.equal(stub.byId("group-" + key).hidden, false, "group-" + key);
  }
  // Card children: status, meta, identity, summary, timing.
  assert.equal(stub.byId("ready-list").children[0].children[4].textContent, "12 ms");
  const attention = stub.byId("attention-list").children;
  assert.equal(attention[0].children[4].textContent, "1.50 s");
  assert.equal(attention[1].children[4].textContent, "999 ms");

  // A surface filter that empties a group updates the count and hides
  // that group, leaving the others untouched.
  stub.byId("tab-rpc").dispatch("click", {});
  assert.equal(stub.byId("ready-heading").textContent, "Ready (0)");
  assert.equal(stub.byId("group-ready").hidden, true);
  assert.equal(stub.byId("attention-heading").textContent, "Attention Required (2)");
  assert.equal(stub.byId("incomplete-heading").textContent, "Incomplete (1)");
  assert.equal(visibleCards(stub), 3);
});

test("the report source opens as a native modal dialog and closes from the button", async () => {
  const stub = dom();
  await loaded(stub, {});
  assert.equal(stub.byId("open-source").hidden, false);

  stub.byId("open-source").dispatch("click", {});
  assert.deepEqual(stub.modalOps, ["show"]);
  assert.equal(stub.byId("report-source").open, true);

  stub.byId("close-source").dispatch("click", {});
  assert.deepEqual(stub.modalOps, ["show", "close"]);
  assert.equal(stub.byId("report-source").open, false);

  // A click on the backdrop (target === dialog element) closes too.
  stub.byId("report-source").open = true;
  stub.byId("report-source").dispatch("click", { target: stub.byId("report-source") });
  assert.equal(stub.byId("report-source").open, false);

  // A click on the dialog's own content must NOT close it.
  stub.byId("report-source").open = true;
  stub.byId("report-source").dispatch("click", { target: stub.byId("report-source-code") });
  assert.equal(stub.byId("report-source").open, true);
  assert.deepEqual(stub.modalOps, ["show", "close", "close"]);
});

test("the theme toggle flips state, persists the choice and survives denied storage", () => {
  const s1 = dom();
  s1.byId("theme").dispatch("click", {});
  assert.equal(s1.documentElement.getAttribute("data-theme"), "dark");
  assert.equal(s1.byId("theme").getAttribute("aria-pressed"), "true");
  assert.equal(s1.storage.get("stellar-canary-viewer:theme"), "dark");

  s1.byId("theme").dispatch("click", {});
  assert.equal(s1.documentElement.getAttribute("data-theme"), "light");
  assert.equal(s1.byId("theme").getAttribute("aria-pressed"), "false");
  assert.equal(s1.storage.get("stellar-canary-viewer:theme"), "light");

  const s2 = createStubDom();
  s2.storage.set("stellar-canary-viewer:theme", "dark");
  s2.runApp();
  assert.equal(s2.documentElement.getAttribute("data-theme"), "dark");
  assert.equal(s2.byId("theme").getAttribute("aria-pressed"), "true");

  const s3 = createStubDom({ systemDark: true, allowStorage: false });
  s3.runApp();
  assert.equal(s3.documentElement.hasAttribute("data-theme"), false, "no forced theme without storage");
  assert.equal(s3.byId("theme").getAttribute("aria-pressed"), "true", "pressed follows the OS in system mode");
  s3.byId("theme").dispatch("click", {});
  assert.equal(s3.documentElement.getAttribute("data-theme"), "light");
  assert.equal(s3.storageAttempts.writes > 0, true, "the write was attempted");
  assert.equal(s3.documentElement.getAttribute("data-theme"), "light", "denied storage never breaks the toggle");
});

test("an invalid report hides the findings view and keeps the reading order", async () => {
  const stub = dom();
  await loaded(stub, {});
  assert.equal(stub.byId("findings").hidden, false);

  await loadReport(stub, "{not json");
  assert.equal(stub.byId("findings").hidden, true);
  assert.equal(stub.byId("skipped-section").hidden, true);
  assert.equal(stub.byId("invalid-reason").hidden, false);
  assert.equal(stub.byId("facts").hidden, true);
});

/* --- WCAG 2.1 contrast of both palettes, computed from the CSS --- */

function channel(c) {
  const x = c / 255;
  return x <= 0.04045 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
}

function luminance(hex) {
  const h = hex.replace("#", "");
  if (!/^[0-9a-f]{6}$/i.test(h)) throw new Error("unsupported colour " + hex);
  return (
    0.2126 * channel(parseInt(h.slice(0, 2), 16)) +
    0.7152 * channel(parseInt(h.slice(2, 4), 16)) +
    0.0722 * channel(parseInt(h.slice(4, 6), 16))
  );
}

function ratio(fg, bg) {
  const a = luminance(fg), b = luminance(bg);
  const hi = Math.max(a, b), lo = Math.min(a, b);
  return (hi + 0.05) / (lo + 0.05);
}

/* Scope extraction: the CSS declares tokens in :root, in the dark media
 * query block and again on :root[data-theme="dark"]; the test treats the
 * two dark declarations as one palette and asserts they never drift. */
function palette(name, css) {
  const out = {};
  for (const m of css.matchAll(/--([a-z-]+):\s*(#[0-9a-f]{6})/gi)) {
    const key = m[1].replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    if (out[key] !== undefined && out[key] !== m[2]) {
      throw new Error(name + ": --" + m[1] + " differs within its scope");
    }
    out[key] = m[2];
  }
  return out;
}

test("the two dark palette declarations are identical (media query and explicit)", () => {
  const css = read("style.css");
  const media = css.match(/@media \(prefers-color-scheme: dark\) \{\s*:root:not\(\[data-theme="light"\]\) \{([^}]*)\}/);
  const explicit = css.match(/:root\[data-theme="dark"\] \{([^}]*)\}/);
  assert.ok(media, "media-scoped dark block present");
  assert.ok(explicit, "explicit dark block present");
  const vars = (s) => [...s.matchAll(/--([a-z-]+):/g)].map((m) => m[1]).sort();
  assert.deepEqual(vars(media[1]), vars(explicit[1]), "same tokens in both blocks");
  for (const m of vars(media[1])) {
    const a = media[1].match(new RegExp(`--${m}:\\s*(#[0-9a-fA-F]{6})`));
    const b = explicit[1].match(new RegExp(`--${m}:\\s*(#[0-9a-fA-F]{6})`));
    assert.equal(a[1].toLowerCase(), b[1].toLowerCase(), "--" + m + " matches across dark blocks");
  }
});

test("every colour pair satisfies WCAG AA in light and dark themes", () => {
  const css = read("style.css");
  const root = palette("light", css.match(/:root \{([^}]*)\}/)[1]);
  const mediaDark = palette("dark", css.match(/:root:not\(\[data-theme="light"\]\) \{([^}]*)\}/)[1]);

  const textMinimum = 4.5;
  const nonTextMinimum = 3;

  for (const [themeName, p] of [["light", root], ["dark (media)", mediaDark]]) {
    for (const bgKey of ["bg", "surface"]) {
      for (const fgKey of ["fg", "muted", "accent", "pass", "warning", "fail", "invalid"]) {
        const r = ratio(p[fgKey], p[bgKey]);
        assert.ok(
          r >= textMinimum,
          `${themeName}: ${fgKey} text on ${bgKey} is ${r.toFixed(2)}:1 (needs ${textMinimum}:1)`,
        );
      }
      assert.ok(
        ratio(p.bannerFg, p.bannerBg) >= textMinimum,
        themeName + ": banner text on banner field reaches AA",
      );
    }
    for (const surface of [p.bg, p.surface]) {
      assert.ok(
        ratio(p.bannerBorder, surface) >= nonTextMinimum,
        `${themeName}: banner border on ${surface === p.bg ? "bg" : "surface"} reaches 3:1`,
      );
      assert.ok(
        ratio(p.border, surface) >= nonTextMinimum,
        `${themeName}: border on ${surface === p.bg ? "bg" : "surface"} reaches 3:1`,
      );
    }
    assert.ok(
      ratio(p.accent, p.bg) >= nonTextMinimum && ratio(p.accent, p.surface) >= nonTextMinimum,
      themeName + ": focus ring (--accent) is visible on both backgrounds",
    );
  }
});
