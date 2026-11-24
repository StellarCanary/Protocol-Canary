// Publishing checks for the viewer bundle (E-090 / CF-08-F). Run with:
//   node --test viewer/test/publish.test.mjs
//
// Asserts that the docs workflow copies viewer/ into docs-site/book/viewer/
// before the link check, that the sidebar and page links exist, that every
// link to the viewer lives on a book-root page (a relative ./viewer/
// reference would 404 inside book/cli/), and that the copied assets keep
// their meta CSP and relative script references.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const viewerDir = path.join(here, "..");
const repoDir = path.join(viewerDir, "..");
const read = (name, base = repoDir) => readFileSync(path.join(base, name), "utf8");

const workflow = read(".github/workflows/docs.yml");
const summary = read("docs-site/src/SUMMARY.md");
const index = read("viewer/index.html");
const jsonReport = read("docs-site/src/json-report.md");

test("the workflow stages the viewer into the book build before building", () => {
  const step = workflow.match(
    /- name: Bundle report viewer\n(\s+run:\s*\|[^\n]*\n([\s\S]*?))\n\s*\n\s+- name: /,
  );
  assert.ok(step, "docs.yml has a 'Bundle report viewer' step");
  const script = step[2];
  assert.match(script, /cp -a viewer docs-site\/src\/viewer/);
  assert.match(script, /test -f docs-site\/src\/viewer\/index\.html/);

  const stageAt = workflow.indexOf("- name: Bundle report viewer");
  const buildAt = workflow.indexOf("- name: Build book");
  assert.ok(buildAt > stageAt, "the viewer is staged before mdbook runs");
});

test("the bundle runs before the link check and before the Pages upload", () => {
  const bundleAt = workflow.indexOf("- name: Bundle report viewer");
  assert.ok(bundleAt > -1);
  const lycheeAt = workflow.indexOf("- name: Check internal links");
  const pagesAt = workflow.indexOf("- name: Upload Pages artifact");
  assert.ok(lycheeAt > bundleAt, "lychee must verify the link that reaches the viewer");
  assert.ok(pagesAt > bundleAt, "the Pages artifact must contain the copied viewer");
});

test("viewer changes alone trigger the docs workflow", () => {
  const paths = workflow.match(/paths:\s*\n\s+- "docs-site\/\*\*"\s*\n\s+- "viewer\/\*\*"\s*\n\s+- "\.github\/workflows\/docs\.yml"/g);
  assert.ok(paths, "on.push.paths and on.pull_request.paths include viewer/**");
  assert.equal(paths.length, 2);
});

test("the sidebar links to the viewer next to the dynamic TOC root", () => {
  // mdbook renders SUMMARY affix links into toc.html, which every page
  // loads from the book root, so the relative ./viewer/ href stays valid
  // on nested pages like book/cli/check.html.
  assert.match(summary, /\[Report Viewer\]\(\.\/viewer\/index\.html\)/);
});

test("page links to the viewer sit on book-root pages only", () => {
  const links = [...jsonReport.matchAll(/\]\(\.\/viewer\/index\.html\)/g)];
  assert.ok(links.length > 0, "json-report.md advertises the viewer");
  for (const file of ["docs-site/src/SUMMARY.md", "docs-site/src/json-report.md"]) {
    assert.ok(
      !path.dirname(file).endsWith("cli"),
      "relative viewer links only work at the book root: " + file,
    );
  }
});

test("the copied viewer assets keep the CSP meta tag and relative references", () => {
  // What the workflow copies is byte-identical to viewer/: the check pins
  // the source file the copy must preserve.
  assert.match(index, /<meta http-equiv="Content-Security-Policy"/);
  for (const m of index.matchAll(/(?:script|link)\b[^>]*\b(?:src|href)="([^"]+)"/g)) {
    const url = m[1];
    assert.doesNotMatch(url, /^([a-z]+:|\/\/|\/)/i, "non-relative asset reference: " + url);
  }
});

test("the viewer page still loads exactly its two local scripts", () => {
  const scripts = [...index.matchAll(/<script src="([^"]+)"><\/script>/g)].map((m) => m[1]);
  assert.deepEqual(scripts, ["report.js", "app.js"]);
});
