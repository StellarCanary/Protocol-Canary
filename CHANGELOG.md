# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

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
