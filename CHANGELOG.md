# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

## [0.2.0] - not yet released

The date is filled in when the tag is created. `0.1.1` stays published and
unchanged.

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
