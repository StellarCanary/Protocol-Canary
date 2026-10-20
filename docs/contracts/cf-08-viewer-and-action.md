# CF-08: Readiness viewer and GitHub Action integration

Status: frozen for implementation planning, contract version 1.
Updated with maintainer decisions D-08b (viewer hosted at `viewer/` beside the book) and D-10 (diagnostics artifacts opt-in and disabled by default).
Authoritative for: the static browser-local report viewer, policy
interpretation, Action inputs and outputs, fixture acquisition, diagnostics and
the cross-repository integration points.

Principles that bind every unit below: the CLI is authoritative for compatibility
decisions; the Action is a thin wrapper; the viewer only displays what a report
says. No backend, database, hosted service or Docker requirement is introduced,
and nothing handles a Stellar secret key or signs or submits a transaction.

## 1. Existing behavior

### Action (`ProtocolCanary-Action`, `action.yml`, `src/`)

Inputs today: `protocol`, `config`, `network`, `rpc-url`, `fixtures-dir`
(default `fixtures`), `version` (default `0.1.1`), `upload-report` (default
`true`), `annotations` (default `true`), `timeout-minutes` (default `15`).
Outputs today: `status` (`pass`, `warning`, `fail`, `error` or
`execution-failed`), `passed`, `warnings`, `failures`, `errors`, `report`
(absolute path, empty on `execution-failed`).

Behavior verified in source: input validation in `src/inputs.ts`; the engine is
installed with `cargo install --git --locked` pinned to the resolved commit (or
tag with a warning), a matching installed or cached binary is reused, and an
installed binary is verified against a published checksum manifest when the
engine release has one (`src/canary.ts`, `src/version.ts`); the report is parsed
and validated in `src/output.ts` (`SUPPORTED_SCHEMA_VERSION = 1`; individual
result entries validated since PR #115); the artifact is named
`stellar-protocol-canary-report` with a fallback name on a 409 (`src/artifact.ts`);
when Canary cannot run or produces an unparseable report the Action sets
`status` to `execution-failed`, writes a failure summary and uploads no report
artifact. The Action never recomputes compatibility; job pass or fail follows the
CLI exit code.

Open Action pull requests on 2026-10-09 (not part of this contract): #321, #288
(same test, issue #251, both conflicting), #320, #302 (same docs, issue #283, both
conflicting), #319 (duplicates merged #115, conflicting, issue #67 already
fixed), #150, #139, #140, #132 (#132 and #140 overlap, issue #33).

### Viewer

No report viewer exists. `docs-site/` is an mdBook documentation site built and
deployed to GitHub Pages by `.github/workflows/docs.yml`
(`docs-site/book/` is the Pages artifact). The planning files that say "Pages
viewer" refer to something not yet created.

## 2. Viewer contract

### 2.1 Delivery

A static page: HTML, CSS and JavaScript in one directory, no build server, no
runtime network request. Approved location `viewer/` in `Protocol-Canary`,
published beside the book at `/viewer/` by extending the existing Pages workflow
(approved under D-08b; maintainer accepts shared origin with dedicated storage keys or prefix). Third-party scripts, fonts,
analytics and CDN requests are forbidden; any library is vendored with its
license and a pinned checksum.

### 2.2 Report input

- The user opens a file with a file picker or drag and drop. The file is read
  with `FileReader` (or `File.text()`); it is never sent anywhere. The page sets a
  Content-Security-Policy meta tag with `default-src 'none'`, `script-src 'self'`,
  `style-src 'self'`, `connect-src 'none'`, `img-src 'self' data:`,
  `base-uri 'none'`, `form-action 'none'`, so a request is blocked even if a bug
  tried to make one.
- The policy is delivered in a `<meta http-equiv="Content-Security-Policy">`
  tag because GitHub Pages cannot set response headers. A meta tag does not
  support `frame-ancestors` or reporting directives; `connect-src 'none'` and
  `script-src 'self'`, which the no-upload guarantee relies on, are enforced.
- Size limit 10 MiB; larger files are refused with a message. Nesting depth is
  limited separately by the browser's `JSON.parse`; the test corpus includes a
  deeply nested report.
- Parsing and validation follow CF-01 exactly: JSON syntax, `schemaVersion === 1`,
  required fields and types, known `status` and `surface` values, no duplicate
  result identity. Unknown extra fields are ignored. Invalid input shows
  "invalid report" with the first reason and displays nothing else as a result.
- `counts` absent: derived from `results`. `counts` present and inconsistent with
  `results`: a visible warning, display the derived counts, keep the report's
  `status` as stated.
- The page persists nothing by default (no localStorage, cookies, IndexedDB).
  A theme preference may be stored in `localStorage` only; reports never are. A
  GitHub Pages project site shares the origin `stellarcanary.github.io` with the
  organization's other Pages sites, so the storage key must be prefixed with `stellar-canary-viewer:`
  (approved under D-08b, accepting the shared origin with isolation).

### 2.3 Policy interpretation

The viewer displays the report's `status`. It does not decide pass or fail from
`results`, does not know `warnings_are_failures`, and does not recompute exit
codes. Readiness wording is fixed by this table:

| Report state | Heading shown | Never shown as |
|---|---|---|
| `status: pass`, `counts.total > 0` | "All executed checks passed" with the count executed and skipped | "Compatible", "Ready", "Supported" |
| `status: pass`, `counts.total == 0` | "No checks ran" with skipped count | any success state |
| `warning` | "Passed with warnings" | success without the word warnings |
| `fail` | "Compatibility failures" | |
| `error` | "Could not complete" | |
| invalid | "Invalid report" | |

Findings are listed per result with surface, id, status, summary and (collapsed)
details. Skipped fixtures are listed with their reason. `network.observedProtocol`
differing from `targetProtocol` is shown prominently. Verification state
(CF-04) is shown only if the report carries it; absence is shown as "no
verification information", never as verified.

All report text is inserted with `textContent` or an equivalent. No `innerHTML`,
no Markdown rendering of report fields, no links made from report strings unless
the string parses as an `https:` URL and is rendered as inert text with an
explicit "open" control.

### 2.4 Comparison view

Two reports may be loaded. The page displays a diff **produced by the same
algorithm as CF-05**, implemented in JavaScript against the shared golden corpus
(CF-05-F), or imports a `diffVersion` 1 file produced by the CLI. A mismatch with
the golden corpus fails the viewer's tests. The viewer never labels an
incomparable pair as compared.

### 2.5 Sharing guidance

A report can contain a project directory name and a branch name. The page states
this before offering any "copy" or "print" action, and exposes no upload feature.
A sanitizing export (strip `git.branch`, `project.name`) may be added and must say
which fields it removed.

## 3. Action contract

### 3.1 Responsibilities

The Action: validates inputs, obtains the engine and (when asked) a fixture pack,
runs `stellar-canary`, parses the JSON report, publishes summary, annotations and
artifact, and sets outputs. It does not: classify results, compute diffs, verify
pack digests or interpret lockfile content itself. For each of those it calls a
CLI command and shows the CLI's answer. Where the CLI lacks the command, the
Action feature waits for the CLI release.

### 3.2 Proposed additional inputs (not implemented)

| Input | Meaning | Rules |
|---|---|---|
| `working-directory` | Passed to the CLI as `--project-root` (CF-07). | Must exist and be a directory; relative values resolve against `GITHUB_WORKSPACE`; `config`, `fixtures-dir`, lock and output paths then resolve against it. |
| `fixtures-pack` | A release version (`1.2.3`) or an archive path. | Mutually exclusive with `fixtures-dir` when both are set explicitly; setting both is an input error that names both. |
| `fixtures-digest` | Expected CF-02 pack digest. | Required when `fixtures-pack` is a release; compared by the CLI. Overrides nothing in a lockfile: if both exist and differ the run fails. |
| `baseline-report` | Path to a previous report. | Comparison via `stellar-canary diff`; missing, malformed or incompatible baseline is reported as such and never as a pass. |
| `require-fixtures` | Forwards the CLI's opt-in zero-result guard. | Only valid if the resolved engine version supports it. |

Every new input has the same validation style as existing ones: an invalid value
is an `InvalidInputError` that quotes the value and the accepted form.

### 3.3 Proposed additional outputs (not implemented)

`fixtures-digest` (the digest the CLI verified), `skipped` (count), `coverage`
(`executed`, `none`), `diff-report` (path), `new-failures` and `resolved` (counts
from the CLI diff). Outputs are strings; an output that does not apply is unset,
not `0`, except where an existing output already documents `0`.

### 3.4 Acquisition order and failure mapping

1. Resolve engine version (existing behavior).
2. Resolve fixtures: `fixtures-dir`, else `fixtures-pack` (download, verify, extract
   per CF-06), else the default `fixtures` directory.
3. If a lockfile exists at the project root, the CLI verifies it (CF-03). Its
   result is shown, not recomputed.
4. Run `check`.
5. If `baseline-report` is set, run `diff`.

| Failure | Action `status` | Job |
|---|---|---|
| Invalid input | unset (today's behavior: `core.setFailed` before any output is set) | fails |
| Pack download or verification failure | `execution-failed`, reason names the step | fails |
| Stale lock (CLI exit per D-04) | `execution-failed`, shows expected and computed digest | fails |
| Report invalid or unparseable | `execution-failed` | fails |
| CLI exit 1 | `fail` | fails |
| CLI exit 3 | `error` | fails |
| CLI exit 0 and `counts.total == 0` | `pass` (as the CLI said), summary headed "No checks ran" | passes |

The last row is deliberate: the Action does not override the CLI. `require-fixtures`
is how a project makes it a failure.

### 3.5 Diagnostics and redaction

When no usable report exists, the Action may upload a diagnostics artifact
(**opt-in and disabled by default, approved under D-10**: an artifact on a
public repository can be downloaded by anyone who can read the repository, and
redaction by pattern is best effort; no diagnostics artifact is uploaded unless
the user explicitly enables it)
containing only: Action version, resolved engine version and commit, the
sanitized command line (no `--rpc-url` query string, no token), the exit code, a
timeout flag, and the last 64 KiB of stdout and stderr with these removed or
masked: the GitHub token, any value of an environment variable whose name
contains `TOKEN`, `SECRET`, `KEY` or `PASSWORD`, and URL userinfo or query
strings. The process environment is never included. The artifact name is distinct
from the report artifact (`stellar-protocol-canary-diagnostics`). Summary and
annotation text escape workflow-command sequences (`::`) and backticks (the
fence handling in `renderExecutionFailureMarkdown` already exists).

### 3.6 Summary size

GitHub limits a step summary to 1 MiB and the number of annotations per step. The
Action must truncate deterministically, say how many entries were omitted, and
point at the report artifact. Truncation never drops the overall status line.

## 4. Cross-repository integration

| Need | Engine | Fixtures | Action | Viewer |
|---|---|---|---|---|
| Report schema | CF-01 emits | none | validates | validates |
| Pack digest | CF-02 computes | CF-02 generates | passes through | shows if present |
| Lock | CF-03 reads and writes | none | surfaces | shows if present |
| Evidence | CF-04 preserves | CF-04 records | shows | shows |
| Diff | CF-05 computes | none | invokes and shows | shows |
| Release pack | CF-06 verifies | CF-06 publishes | downloads | none |
| Roots | CF-07 | none | `working-directory` | none |

Compatibility between versions: the Action pins an exact engine version. A table
in the Action README lists exact verified combinations only (engine version,
Action version, pack version), as today. Features that need a newer engine are
gated by a minimum-engine-version check that fails with a message naming the
needed version, not by guessing.

## 5. Security and trust boundaries

Everything read from a report, a pack, a baseline or a project directory is
untrusted. The viewer cannot make a network request (CSP) and stores nothing.
The Action's token use is limited to reading public release metadata and, when the
user supplies one, a private pack; the token is never logged or written to an
artifact. No component executes content from a pack or a report.

## 6. Validation and test requirements

- Viewer: tests with a malicious report (HTML in every string field, huge strings,
  deeply nested JSON, duplicate identities, unknown enums, `schemaVersion` 2),
  asserting no DOM injection and a refusal where CF-01 says refuse. A test that
  no network request is attempted. Keyboard and screen-reader checks of the
  report list. These run in CI with a headless browser; no hosted service.
- Action: unit tests per new input and output; end-to-end tests using the
  existing mock engine (`tests/fixtures/mock-canary.cjs`) and, separately, one
  test against a real pinned engine binary.
- Cross-repo: a test that every real report sample from the engine corpus
  (CF-01-C) is accepted by both the Action parser and the viewer parser.

## 7. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CF-08-A Viewer shell: file import, CSP, size limit, validation per CF-01 | Protocol-Canary | CF-01 |
| CF-08-B Result list, filters, skipped list, headings from 2.3 | Protocol-Canary | CF-08-A |
| CF-08-C Network and protocol mismatch panel | Protocol-Canary | CF-08-A |
| CF-08-D Verification and fixture identity display (when fields exist) | Protocol-Canary | CF-08-A, CF-04-D |
| CF-08-E Accessibility pass and automated checks | Protocol-Canary | CF-08-B |
| CF-08-F Pages deployment of the viewer, with link check | Protocol-Canary | CF-08-A, D-08b |
| CF-08-G Two-report comparison view with the golden corpus | Protocol-Canary | CF-05-F, CF-08-B |
| CF-08-H Malicious-report and no-network regression tests | Protocol-Canary | CF-08-A |
| CF-08-I Action diagnostics artifact with redaction | ProtocolCanary-Action | none |
| CF-08-J Action `working-directory` | ProtocolCanary-Action | CF-07-A released |
| CF-08-K Action `fixtures-pack` and digest inputs | ProtocolCanary-Action | CF-06-F released |
| CF-08-L Action baseline input and diff summary | ProtocolCanary-Action | CF-05-E released |
| CF-08-M Action summary truncation | ProtocolCanary-Action | none |
| CF-08-N Action reserved-field tolerance and duplicate-identity rejection | ProtocolCanary-Action | CF-01 |
