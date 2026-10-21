# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Added

- XDR fixtures can compare a decoded value to expected JSON with
  `kind = "decode-json-equals"` and an `expected_json` JSON string. The value
  is converted with `stellar-xdr`'s own serde JSON form and compared
  structurally (object key order is ignored, everything else is exact); a
  mismatch lists each difference by JSON Pointer path with the expected and
  actual values, and malformed `expected_json` is an invalid fixture.
  Existing fixture kinds are unchanged.

- `stellar-canary check --fixture <ID>` and `--surface <xdr|rpc|soroban>` to
  run a targeted subset of fixtures. The filters may be combined, preserve
  fixture order, and report an error when no fixture matches.
- `stellar-canary inspect --format json` (and the `--json` shorthand): the
  versioned `inspectVersion: 1` document of CA-03 — project type and
  capabilities, the effective configuration, the target protocol and its
  source, and the fixture plan with `wouldRun`, `skipped` (with the stable
  `code`), `checkWouldRefuse` and `refusal`. The command stays offline and the
  `terminal` output is unchanged, byte for byte.
- The inspect/check parity suite (issue #485, unit CA-03-D): asserts that the
  ids *and order* in `inspect`'s `plan.wouldRun` equal the `results` a
  `check --format json` run executes, that skips match too, and that a
  discrepancy fails with a message naming both lists.
- CF-04 verification evidence (first part of #443): fixtures may carry a
  `[verification]` table with a `verificationVersion: 1` record (`method`,
  `checkedAt`, optional `network`/`verifier`/`endpointHost`/`evidenceRef`,
  `outcome`). The table is validated strictly against CF-04 section 3 (unknown
  keys, a mismatched `fixtureId`, a live method without `network`, a malformed
  `checkedAt` are invalid fixtures, exit 4) and is lifted out of the
  surface-specific body, so no surface runner ever sees it.
- `stellar-canary fixtures --format json` (and the `--json` shorthand): the
  `inventoryVersion: 1` inventory of CA-02 — counts, `otherProtocols`, fixtures
  sorted by id with `/`-relative paths, per-surface fields, and the reserved
  `verification` object copied as stored (issue #444, CA-02-E). The terminal
  listing is unchanged.
- Verification badges in the terminal report (issue #444): when a run's
  fixtures carry evidence, each result row shows `[verified: <method>]`,
  `[unverified: <method> record mismatched]` or `[unverified]` — the unknown
  state is displayed, never omitted. Runs without evidence keep the compact
  `N/N PASS` output.
- `--project-root <PATH>` on `check`, `inspect` and `fixtures` (CF-07, D-08).
  Relative `--config`, `--fixtures-dir` and `--output` paths resolve against
  it. A path that does not exist or is not a directory exits `2`. Without the
  flag nothing changes, and parent directories are still never searched.
- JSON reports gain an optional `run.toolBuildCommit`: the Git commit the
  binary was built from, or `null` when it was built without Git history
  (CF-01). `schemaVersion` stays `1`. No timestamp is recorded, so two builds
  of one commit stay byte-for-byte comparable.
- A strict parser and deterministic serializer for `.stellar-canary.lock`
  (CF-03) in `canary-config`: `load_lock`, `parse_lock` and `serialize_lock`.
  No command reads or writes a lockfile yet.
- JSON reports gain an optional `skipped[].code` (CF-01-D): the stable
  machine-readable skip reason beside the free-form `reason` —
  `UNSUPPORTED_PROTOCOL`, `SURFACE_DISABLED`, `MISSING_CAPABILITY`, `OFFLINE`, or
  `UNKNOWN` when the reason is not recognized (`NETWORK_MISMATCH` is
  reserved for a future network-identity skip). `schemaVersion` stays `1`;
  older reports lack the field, which reads back as `UNKNOWN`, and
  consumers must ignore codes they do not know.
- Common execution errors print an actionable `Hint:` line under the
  `error:` line on stderr: connection failures (check that the local
  standalone node or RPC endpoint is running and reachable), permission
  denied (check the file mode), a stale or unreadable lockfile, a report
  schema mismatch (use the engine version that wrote it), and a missing
  or unreadable project configuration (create `.stellar-canary.toml` or
  fix `--config`). An error with no advice to give keeps printing
  exactly one clean error line. Error wording, categories and exit codes
  are unchanged.
- The formal JSON Schema for the version 1 report (CF-01-A/CF-01-B) is
  published at `docs/schemas/report-v1.schema.json` (draft 2020-12). It
  requires exactly the fields CF-01 section 3 requires, pins the closed
  sets (`status`, `surface`, `project.type`) to their current values and
  ignores unknown properties, so an additively extended report still
  validates. Conformance tests validate real rendered reports and the
  CF-01 examples against it, and reject reports missing required sections
  or carrying invalid statuses.
- `--rpc-max-retries <N>` (default `3`) and `--rpc-retry-delay-ms <MS>`
  (default `500`) on `check`: how many times a failed RPC attempt may be
  retried and the linear-backoff base delay between attempts. `0` retries
  means one attempt and failure on the first network error. The policy is
  applied to every RPC call the run makes — the network probe and the
  fixture executions alike — through `RetryPolicy::from_retries` in
  `canary-rpc`.
- `--cache-dir <PATH>` on `check`: where the local result cache lives, for
  example a CI runner's persistent cache or a tmpfs mount. The directory is
  created if it does not exist, and a path that cannot be created exits `2`
  before any fixture runs with an error naming the flag and the path. A
  relative path resolves against `--project-root` when that flag is given.
  Without the flag the cache stays in `.stellar-canary-cache` in the project
  root.
- `canary_fixtures::pack_digest`: the `canary-pack-v1` digest of a fixture
  pack (CF-02), with the file list it was computed from. It is distinct from
  the engine's internal per-fixture cache key. No command uses it yet.
- RPC fixtures accept a `[request]` table for methods that take parameters.
  It is validated when the fixture is loaded against the CA-01 parameter
  table (unknown key, wrong type, out-of-range value, `xdrFormat = "json"`
  and `[request]` on a parameterless method are invalid fixtures, exit `4`).
  The engine still runs only `get-network` and `get-latest-ledger`.
- `canary_report::diff`: the report comparison model, status transition
  table, eligibility checks and `diffVersion` 1 JSON (CF-05). A library
  only; there is no `diff` subcommand yet.
- `viewer/`: a static, browser-local report viewer shell with a strict
  Content-Security-Policy (CF-08). Not published to GitHub Pages yet.
- Soroban fixtures accept `bytes` (a `0x`-prefixed hex string, otherwise
  padded base64), `address` (a `G...` account or `C...` contract strkey),
  `vec` and `map` arguments. `vec` and `map` nest up to 8 levels deep and a
  map is encoded with its keys sorted; a repeated key is rejected. A secret
  seed given as an address is rejected and never echoed.
- Cargo workspace members are detected (CF-07): `[workspace] members` and
  `exclude` are read with `*` and `?` globs up to 4 levels deep, at most 64
  members are scanned (a boundary diagnostic is returned beyond that), and a
  virtual workspace whose members use `soroban-sdk` is now detected as a
  Soroban project instead of `Unknown`. Symbolic links are not followed.
- `stellar-canary completions <bash|zsh|fish|powershell>` (issue #500): prints
  a shell completion script on stdout, generated from the same clap command
  definition the binary parses with, so the script always describes the real
  subcommands, options and flags of the installed version. Stdout carries only
  the script — diagnostics go to stderr — so it can be redirected to a file or
  sourced from a shell configuration. An unsupported shell name is rejected by
  argument parsing with exit code `2` and an error on stderr that lists the
  four supported shells; no script is emitted.

### Changed

- RPC fixture assertions are evaluated against the raw JSON `result` the
  endpoint returned, not a typed projection of it (CA-01 section 4.1). A
  field the engine does not model can now be asserted on, and `field-absent`
  on such a field is evaluated truthfully instead of passing for the wrong
  reason. A `result` that is not an object is an `error`, not a failed
  assertion. The shipped Protocol 28 fixtures give the same outcome.
- `--output` now writes the report file atomically: the bytes go to a
  `.tmp` sibling in the target directory, which is then renamed over the
  destination, so a killed run can no longer leave a truncated or
  unparseable report on disk. An existing report's permissions are
  preserved, a failed write leaves no `.tmp` file behind, and the written
  bytes are unchanged (still exactly what stdout prints).
- The statically hosted report viewer (CF-08-E, #463): loading a report
  now shows findings and skipped fixtures as an accessible list —
  surface filters as a `role="tablist"` tabs pattern with
  `aria-selected` and a roving tabindex moved by the arrow keys, Home
  and End, a labelled search box whose `aria-live="polite"` region
  announces filtered counts and the empty state, collapsed `details`
  per result, and a native modal `<dialog>` with the raw report text
  that traps focus and returns it to the trigger on close. The theme
  button reports its state with `aria-pressed`, and keyboard focus is
  a 3px ring in both themes.
- Themes in the viewer (E-088, #464): a `--surface` token joins the
  palette and every colour pair is verified to WCAG AA by the test
  suite — text at 4.5:1 on both backgrounds, borders and focus rings
  at 3:1. The OS preference is the default (`prefers-color-scheme`;
  both dark blocks are identical), an explicit toggle persists under
  `stellar-canary-viewer:theme`, and denied storage falls back to the
  OS theme without failing.
- Security regression tests for the viewer (CF-08-H, #465): a corpus
  with XSS payloads in every string field, oversized strings, deeply
  nested JSON, duplicate identities, unknown enums and a wrong
  `schemaVersion`. Rendering runs `app.js` against a stub DOM that
  throws on any HTML sink, proving hostile text stays inert inside
  text nodes, no URL-bearing attribute is written, a 10 MiB file is
  refused before being read, and no network API is used (the page
  ships `connect-src 'none'`). Runs in CI with the other viewer suites.
- The documentation workflow stages the viewer into `docs-site/src/viewer/`
  before the mdBook build, so the Pages site serves it at `/viewer/`
  beside the book (CF-08-F, #466). The summary gains an unnumbered
  "Report Viewer" sidebar entry, the JSON Report page cross-links it,
  the link check covers the new link, and `create-missing = false`
  makes a missing viewer bundle fail the build loudly.

## [0.2.0] - 2026-10-09

`0.1.1` stays published and unchanged.

### Upgrade notes

Read these before moving a CI job from `0.1.1`.

1. **A run that executes nothing now fails.** `check` exits `2` and prints no
   report when no fixture applies (missing or empty fixtures directory, or every
   fixture skipped by protocol, disabled surface or missing capability). Add
   `--allow-empty` only if an empty run is intended. This is the reason for a
   minor version.
2. **Fixture directories must not contain symbolic links, junctions or unsafe
   `input_file`/`expected_file` paths.** They are rejected with exit `4`.
   The canonical Protocol 28 pack (tag `protocol-28` and current `main` of
   `ProtocolCanary-Fixtures`) contains none and loads unchanged; CI checks both.
3. **`check` now keeps a result cache in `.stellar-canary-cache/` in the project
   root.** Only offline XDR results are replayed by default; RPC and Soroban
   results are replayed only with `--live-cache-ttl`. Add the directory to
   `.gitignore`, or pass `--no-cache`.
4. **JSON reports gain an optional `results[].source`** (`live` or `cache`).
   `schemaVersion` stays `1`; consumers that ignore unknown fields are unaffected.
5. **`.stellar-canary.toml` now rejects unknown sections and keys** with exit
   code `2` and the field name. Before, a typo such as `[test]` for `[tests]`
   was ignored and left every surface enabled. A config that only used the
   documented keys is unaffected; the example configs in this repository load.
6. **Fixtures are checked strictly.** An unknown key in a fixture body, an `rpc`
   fixture with no `[[assert]]`, or a misspelled assertion table is an invalid
   fixture (exit `4`), where before the fixture ran and could pass while checking
   nothing. A fixture body the engine cannot read is now exit `4` instead of `3`,
   as `docs/fixture-contract.md` always said. The canonical Protocol 28 pack is
   unaffected (CI checks the tag and `main`).
7. The two Protocol 28 RPC identity fixtures fail against a network that reports
   protocol 29. That is correct and unchanged by this release. This release does
   not add Protocol 29 support.

### Changes

- **Behavior change from `0.1.1`:** `check` now fails when
  it would execute zero fixtures. `0.1.1` and earlier reported such a run as
  `Status: PASS` with `0/0 applicable checks` and exit code `0`, so a missing
  fixtures directory, a wrong `--fixtures-dir`, or `--protocol 29` against a
  pack that only has Protocol 28 fixtures all looked like success. The run now
  exits `2` before executing anything and prints no report, with an error
  naming the cause: the directory is missing, it holds no `*.toml` files, or
  every loaded fixture was skipped (target protocol, disabled surface, missing
  project capability). New `--allow-empty` keeps the old exit code for runs
  that are meant to be empty; it prints a warning and the report still shows
  `counts.total: 0`. The JSON report shape is unchanged. Because this turns a
  previously passing invocation into a failing one, it ships in a minor
  version (`0.2.0`). Scripts and CI that rely
  on an empty run passing need `--allow-empty` or, better, a fixtures path
  that matches the target protocol. The GitHub Action pins an engine version
  and only sees this once its default `version` moves to the release that
  contains it.
- **The local result cache is now used by `check`.** `0.1.0` and `0.1.1`
  created the cache type but never consulted it (the "Known gaps" entry under
  `0.1.0` was accurate for both). It was wired in after `0.1.1`, and the
  version released here includes three corrections made before any release:
  - The cache key covers the fixture's contents (the fixture file and any
    `input_file` or `expected_file` it references), the network name, the tool
    version and a path-free project fingerprint (project type, capabilities
    and Git state). While the wiring was unreleased, an edited fixture could
    return the previous result; that never shipped. File names are a SHA-256
    of the full key, and each entry records a layout number and its key.
  - Only offline XDR results are replayed by default. RPC and Soroban results
    are neither stored nor served unless `--live-cache-ttl <SECONDS>` is given,
    and then only while younger than that. `--no-cache` skips the cache.
  - `results[].source` in the JSON report is `"live"` or `"cache"` (absent in
    older reports, meaning not recorded). The terminal and Markdown reports say
    how many results were replayed.

  Visible effect: `check` creates a `.stellar-canary-cache/` directory in the
  project root. It is a cache and safe to delete; add it to `.gitignore`.
- Fixture loading is stricter, and the same on every platform. A symbolic
  link or junction anywhere under `--fixtures-dir` is now an invalid fixture
  (exit 4) instead of being followed, a `.git` directory is no longer
  entered, and an `input_file` or `expected_file` that is absolute, drive
  qualified, contains a backslash or has an empty, `.` or `..` segment is
  rejected when the fixture is parsed. A fixture directory that relied on
  links or on `..` references must be flattened into real files.
- `canary-core` no longer exports the unused `CompatibilityTest` trait and
  `CompatibilityPlanner` marker type; the `planner` module is removed and
  `engine` now exports only `ExecutionContext`. Surface runners implement
  their own execution traits, and fixture planning lives in
  `canary-runner`'s scheduler (`build_plan`).
- `RpcError::NetworkMismatch` and `RpcError::ProtocolMismatch` are now
  actively raised: `canary_rpc::validate_network_info` constructs them
  from a `getNetwork` response, and `check` compares the observed network
  identity against `--network`/`--protocol`. A passphrase mismatch aborts
  as a configuration error (exit 2); an observed-protocol mismatch prints
  a `warning:` line on stderr and the run continues (the report's
  `(observed protocol N)` annotation is unchanged).
- `.stellar-canary.toml` is parsed strictly: unknown sections and keys are
  errors (`#[serde(deny_unknown_fields)]` on every config table), so a
  misspelled `[tests]` can no longer silently run network checks.
- Fixture bodies are parsed strictly (unknown keys, empty RPC assertion lists) and
  reported as exit `4`; see the upgrade notes.
- Internal: Windows junction and symbolic link tests for the fixture loader, a
  CI job that runs the engine against the canonical Protocol 28 pack (tag and
  `main`), and a release workflow check that the tag equals the workspace
  version.
- `stellar-canary cache clear`: deletes the cached fixture execution results
  under `.stellar-canary-cache` in the project root (or `--cache-dir`) and
  reports the reclaimed disk space in a human-readable form. An empty or
  missing cache directory reports zero bytes and exits successfully (issue
  #490).

## [0.1.1]

- `canary-xdr` now also supports the `"ContractExecutable"` XDR type name
  (previously only `"StellarValue"`), needed by `ProtocolCanary-Fixtures`
  to test CAP-0085's `CONTRACT_EXECUTABLE_EXTERNAL_REF` case.

## [0.1.0]

Initial release.

- `stellar-canary check`: config → project detection → fixture loading/
  validation → compatibility planner → XDR/RPC/Soroban runners → policy
  evaluation → terminal/JSON/Markdown report → documented exit code
  (0 pass, 1 compatibility failure, 2 configuration error, 3 execution/
  RPC error, 4 invalid fixture, 5 internal error).
- `stellar-canary inspect`, `stellar-canary fixtures`, `stellar-canary
  report`, `stellar-canary version`.
- XDR compatibility via the official `stellar-xdr` 28.0.0 crate
  (decode-success, decode-failure, roundtrip, encode-equals).
- A Stellar RPC client (`getNetwork`, `getLatestLedger`,
  `simulateTransaction`) with bounded retries and field-shape assertions.
- Soroban compatibility via unsigned `InvokeHostFunction` transaction
  construction and simulation — no private key is ever read, and no
  transaction is ever submitted.
- Three real Protocol 28 fixtures (one per surface) under
  `tests/fixtures/protocol-28`, verified against
  `soroban-testnet.stellar.org`; see `docs/protocol-28.md`.
- Git metadata (commit/branch/dirty status) and end-to-end test coverage
  for every documented exit code.

### Known gaps

- `canary_core::CacheStore` (a local, file-backed, per-fixture result
  cache keyed by fixture id/protocol/project fingerprint/RPC endpoint/
  observed protocol) is implemented and unit-tested, but not yet wired
  into `check`'s execution path — every run currently calls RPC/Soroban
  fresh. Not required by this project's own MVP definition, but worth
  doing before relying on this tool to avoid rate limits at scale.
