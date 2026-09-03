# Architecture

This document explains how the three `StellarCanary` repositories fit
together and how a single `stellar-canary check` run flows through the
system. It is a map, not a tutorial — see each repository's own README and
`docs/` for implementation detail.

## Purpose

Protocol Canary checks whether a Stellar application or piece of
infrastructure remains compatible with a target Stellar protocol version,
across three surfaces: XDR encoding/decoding, Stellar RPC responses, and
Soroban transaction simulation. It evaluates explicit, declared
compatibility assertions ("fixtures") — it does not prove an application
is compatible in any general sense, and it does not submit transactions.

## Component responsibilities

| Repository | Responsibility |
|---|---|
| [`Protocol-Canary`](https://github.com/StellarCanary/Protocol-Canary) | The compatibility engine: the `stellar-canary` CLI, project detection, fixture loading/validation, the XDR/RPC/Soroban runners, policy evaluation, and terminal/JSON/Markdown reporting. |
| [`ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures) | Canonical, versioned compatibility assertions ("fixtures") for each protocol version. Declarative TOML data only — no business logic. |
| [`ProtocolCanary-Action`](https://github.com/StellarCanary/ProtocolCanary-Action) | GitHub Actions integration. A thin wrapper that installs a pinned `Protocol-Canary` release, runs it, and turns its JSON report into a job summary, annotations, and an artifact. |

## Dependency direction

```text
ProtocolCanary-Fixtures
        |
        v
   Protocol-Canary
        |
        v
ProtocolCanary-Action
```

`Protocol-Canary` depends on nothing outside itself to run — it can load
fixtures from any local directory, including a `ProtocolCanary-Fixtures`
checkout. `ProtocolCanary-Action` depends on a released `Protocol-Canary`
version and, in its examples and live-integration workflow, a
`ProtocolCanary-Fixtures` checkout. Neither `Protocol-Canary` nor
`ProtocolCanary-Fixtures` depends on the Action — both are usable standalone
from the command line.

## Execution flow (GitHub Actions)

```text
Repository under test
        |
        v
ProtocolCanary-Action        (installs a pinned Protocol-Canary release)
        |
        v
Protocol-Canary               (stellar-canary check --format json)
        |
        v
ProtocolCanary-Fixtures       (--fixtures-dir points at a checkout)
        |
        v
XDR / RPC / Soroban checks     (canary-xdr / canary-rpc / canary-soroban)
        |
        v
JSON report                   (schemaVersion 1)
        |
        v
GitHub summary / annotations / artifact
```

The same flow runs locally without the Action: a developer clones
`ProtocolCanary-Fixtures`, installs `Protocol-Canary`, and runs
`stellar-canary check --fixtures-dir <fixtures checkout>` directly.

## Data flow inside `Protocol-Canary`

1. **Config** — `.stellar-canary.toml` (or CLI flags) determines the target
   protocol, network, RPC endpoint, and which surfaces are enabled.
2. **Project detection** — `canary-project` inspects the current directory
   to classify it (`soroban`, `rpc-consumer`, `stellar-sdk`,
   `generic-stellar`, or `unknown`), which determines which fixtures'
   `required_capabilities` are satisfied.
3. **Fixture loading** — `canary-fixtures` recursively loads every `.toml`
   file under `--fixtures-dir` and validates the whole set (unique IDs,
   resolvable file references, schema conformance).
4. **Planning** — `canary-runner`'s planner filters fixtures to the ones
   whose declared `protocol` matches the run's target and whose
   `required_capabilities` the detected project satisfies; everything else
   is recorded as skipped, with a reason, not failed.
5. **Execution** — each planned fixture runs through the surface crate that
   understands its `surface` field: `canary-xdr` (decode/encode/roundtrip
   against the official `stellar-xdr` crate), `canary-rpc` (live calls
   against the configured RPC endpoint), or `canary-soroban` (unsigned
   transaction construction and `simulateTransaction`).
6. **Policy** — `canary-core`'s policy evaluator turns the set of
   per-fixture results into one overall `status` (`pass` / `warning` /
   `fail` / `error`) and the matching process exit code.
7. **Reporting** — `canary-report` renders the same result set as terminal
   output, JSON (`schemaVersion: 1`), or Markdown.

## Trust boundaries

- **Fixtures are untrusted data, not code.** `ProtocolCanary-Fixtures`
  fixtures (and any third-party fixture directory) are declarative TOML;
  there is no field that executes a shell command, script, or arbitrary
  code. See each repository's `SECURITY.md`.
- **No private keys.** Nothing in this system ever reads, stores, or
  transmits a private key or seed phrase. The Soroban surface builds and
  simulates unsigned transactions only.
- **No transaction submission.** Every network interaction is read-only
  (`getNetwork`, `getLatestLedger`) or simulation-only
  (`simulateTransaction`). Nothing in the system submits a transaction to
  any network.
- **The Action never reinterprets a result.** `ProtocolCanary-Action`
  parses the CLI's JSON and passes or fails the job according to the CLI's
  own exit code / `status` field — it does not recompute compatibility
  itself. Backward compatibility with pre-`counts` reports is handled by
  deriving `counts` from `results` when the field is absent, not by
  guessing at pass/fail.

## Release relationship

`ProtocolCanary-Action` pins a specific `Protocol-Canary` version (input
`version`, currently defaulting to `0.1.1`) rather than tracking `main`.
Upgrading the default requires a new Action release and an entry in its
README's "Supported Canary versions" table. See
[Version compatibility](../README.md#version-compatibility) for the
current verified combination.

## Report generation and exit codes

See [`json-report-contract.md`](json-report-contract.md) for the full JSON
schema and [`fixture-contract.md`](fixture-contract.md) for how fixtures
are loaded and validated. Exit codes: `0` pass, `1` compatibility failure,
`2` configuration error, `3` execution/RPC error, `4` invalid fixture, `5`
internal error (`crates/canary-core/src/errors.rs`).
