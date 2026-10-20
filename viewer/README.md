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

## Not in this unit

The per-result list and filters, the comparison view, publishing under
`/viewer/` on GitHub Pages and the malicious-report corpus are separate units
of CF-08.

## Tests

```sh
node --test viewer/test/viewer.test.mjs
```

The tests need only Node.js 18 or later. They cover validation, the heading
table, the size limit, and static checks on the shipped files: the CSP
directives, no external URLs, no inline script or style, no `innerHTML`, no
network API, and a single prefixed storage key.
