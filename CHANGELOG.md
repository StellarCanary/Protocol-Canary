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
3. **RPC and Soroban results are no longer replayed from the local cache unless
   `--live-cache-ttl` is given.** Old cache entries are ignored.
4. **JSON reports gain an optional `results[].source`** (`live` or `cache`).
   `schemaVersion` stays `1`; consumers that ignore unknown fields are unaffected.
5. The two Protocol 28 RPC identity fixtures fail against a network that reports
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
- Replayed results are now marked and live results are no longer replayed by
  default. `results[].source` in the JSON report is `"live"` or `"cache"`
  (absent in older reports, meaning not recorded). `check` no longer serves
  RPC or Soroban results from the cache unless `--live-cache-ttl <SECONDS>`
  is given, and then only while the entry is younger than that. XDR results,
  which are offline and fully determined by the cache key, are still cached.
  New `--no-cache` skips the cache entirely. The terminal and Markdown
  reports state how many results were replayed. Cache entries now carry their
  creation time and a layout number; entries from earlier layouts are never
  read.
- The result cache key now includes the fixture's contents (the fixture file
  and any `input_file` or `expected_file` it references), the network name,
  the tool version and a path-free project fingerprint (project type,
  capabilities and Git state). Before this, editing a fixture and running
  `check` again could return the previous result. Cache file names are now a
  SHA-256 of the full key instead of a lossy, partly hashed name, so fixture
  ids that differ only in punctuation no longer share a file. Entries written
  by `0.1.1` use a different layout and are never read; they can be deleted
  with the rest of `.stellar-canary-cache`.
- Fixture loading is stricter, and the same on every platform. A symbolic
  link or junction anywhere under `--fixtures-dir` is now an invalid fixture
  (exit 4) instead of being followed, a `.git` directory is no longer
  entered, and an `input_file` or `expected_file` that is absolute, drive
  qualified, contains a backslash or has an empty, `.` or `..` segment is
  rejected when the fixture is parsed. A fixture directory that relied on
  links or on `..` references must be flattened into real files.
- Documentation: the README, `ROADMAP.md`, `docs/architecture.md` and the
  mdBook said the result cache was not wired into `check`. It has been wired
  since `0.1.0` (`crates/canary-cli/src/commands.rs`). The text now describes
  the actual behavior and its known limits (key ignores fixture content, no
  expiry, replayed results are not marked in the report). The "Known gaps"
  entry under `0.1.0` below is left as the historical record.
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
