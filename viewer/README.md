# Report viewer

A static, browser-local viewer for `stellar-canary` JSON reports (contract
CF-08, unit CF-08-A). It is plain HTML, CSS and JavaScript with no build step
and no third-party code.

Open `index.html` in a browser, or serve this directory with any static file
server, then choose a report file.

## What it guarantees

- The file is read in the tab and never sent anywhere. The page sets
  `connect-src 'none'` in a Content-Security-Policy meta tag, so a request is
  blocked even if a bug tried to make one.
- Report text is inserted with `textContent` only.
- Nothing is persisted except the theme preference, in `localStorage` under the
  `stellar-canary-viewer:` prefix.
- Files over 10 MiB are refused before they are read.
- Validation follows the report schema (`schemaVersion` 1): required fields and
  types, known `status` and `surface` values, no repeated result identity.
  Unknown extra fields are ignored. An invalid file shows "Invalid report" and
  the first reason, and nothing else.
- The heading is chosen from the report's own `status` and is never
  "Compatible", "Ready" or "Supported".
- When `network.observedProtocol` differs from `targetProtocol`, the report
  opens with a high-contrast alert banner naming both versions ("Protocol
  Mismatch Warning: Target protocol (…) differs from network observed
  protocol (…)."). Reports whose protocols match, and reports that never
  observed a protocol, show no banner.
- Every fixture identity in the findings and skipped lists is a button that
  opens a detail dialog: the fixture SHA-256, the pack digest, the protocol
  numbers, the verification record (method, outcome, checked-at, verifier,
  endpoint host, evidence reference) and, when an inventory is loaded, the
  category, description and required capabilities. The dialog always shows
  the same rows, so a value the report does not carry reads as a sentence
  ("no verification record in this report") and never as a verified fixture.
  Nothing is invented: `fixturePack`, `results[].verification` and the
  CA-02 inventory fields are optional, and an absent one stays absent.
- The fixture inventory (`stellar-canary fixtures --format json`) is an
  optional second file, chosen in the same tab and subject to the same size
  limit and read-in-place rule. A report alone renders the whole dialog; the
  inventory only adds the fields the report cannot carry. Where both carry a
  value, the report wins.
- The dialog offers a copy control for the SHA-256 and the pack digest, and
  only when the browser gives the page clipboard access. Copying puts one
  value on the clipboard; it is the only side effect the page can have, and
  the page says so before offering the control.

## Accessibility and themes

Interactive controls are reachable by keyboard only. The surface filters
are a WAI-ARIA tabs pattern (`role="tablist"`, `role="tab"`,
`role="tabpanel"`, `aria-selected`, roving tabindex, Arrow keys, Home and
End), Enter and Space activate every button and the `<details>` toggles
natively, and the raw report and the fixture detail both open in a native
modal `<dialog>` that traps Tab focus, closes on Escape and on a click
outside the panel, and returns focus to the trigger button on close. Each
fixture button carries an `aria-label` naming its surface and identity, and
the dialog's copy controls do too, so nothing is labelled by the single
word "Copy". The search results area is an `aria-live="polite"` region
announcing the filtered counts and the empty state, and the copy result is
announced through a `role="status"` region. Focus is drawn as a 3px ring in
both themes, and every colour pair is verified against WCAG AA by the test
suite (text at 4.5:1, borders and focus at 3:1).

The theme is defined entirely by CSS custom properties: `:root` holds the
light palette, `@media (prefers-color-scheme: dark)` and
`:root[data-theme="dark"]` hold the identical dark palette, so the OS
preference is the default while an explicit toggle wins. The user's
choice persists in `localStorage` under `stellar-canary-viewer:theme`;
if storage is unavailable (private browsing) the page follows the OS
theme instead of failing.

## Not in this unit

The viewer never fetches a report from a URL, a GitHub run or an RPC
endpoint: every file is one you chose in this tab. It does not write a
report, change a fixture, or re-run a check, and it does not derive
verification freshness — the engine treats a record's age as a derived
property, so the dialog shows the record's own `checkedAt` and draws no
conclusion from it. A report that carries no verification record is
reported as unknown, never as verified.

## Tests

```sh
node --test viewer/test/viewer.test.mjs
node --test viewer/test/security.test.mjs
node --test viewer/test/a11y.test.mjs
node --test viewer/test/publish.test.mjs
```

The tests need only Node.js 18 or later and run in the `viewer` job of
`.github/workflows/ci.yml`. They cover validation, the heading table,
the size limit, the transition matrix, the fixture detail dialog and its
inventory join, and static checks on the shipped files: the CSP
directives, no external URLs, no inline script or style, no
`innerHTML`, no network API, and a single prefixed storage key.

- `security.test.mjs` (CF-08-H) loads report files whose every string
  field carries XSS payloads — script tags, HTML, event handlers, a
  tracking pixel, `javascript:` URLs — plus oversized strings, deeply
  nested JSON, duplicate identities, unknown enums and a wrong
  `schemaVersion`. It asserts the hostile text survives verbatim inside
  text nodes, that no element/attribute injection happens at runtime
  (the stub DOM throws on contact with `innerHTML` or non-allowlisted
  `createElement`), that a 10 MiB file is refused before being read,
  that no network API is reachable or used, that a hostile inventory
  cannot certify a fixture or overwrite a verification record, and that
  denied `localStorage` degrades to the OS theme without crashing.
- `a11y.test.mjs` (CF-08-E) drives `app.js` through the stub DOM:
  keyboard operation of the tabs pattern, live search announcements,
  dialog open/close (Escape, the Close button and a backdrop click),
  the copy control with and without clipboard access, theme persistence,
  and the WCAG AA contrast of every palette pair, computed from the
  shipped CSS.
- `publish.test.mjs` pins the pages deployment: the docs workflow must
  bundle `viewer/` into `docs-site/book/viewer/` before the link check,
  the sidebar and page links must exist, and the assets must stay
  relative with their CSP preserved.
