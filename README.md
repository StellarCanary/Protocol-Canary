# Stellar Protocol Canary

[![CI](https://github.com/StellarCanary/Protocol-Canary/actions/workflows/ci.yml/badge.svg)](https://github.com/StellarCanary/Protocol-Canary/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Rehearse Stellar protocol upgrades before they reach your production stack.

## What is Protocol Canary?

Stellar Protocol Canary is a command-line tool that checks whether a Stellar
application or piece of infrastructure remains compatible with a target
Stellar protocol version. It runs a set of deterministic compatibility
assertions ("fixtures") against three surfaces a Stellar project typically
depends on: XDR encoding/decoding, Stellar RPC responses, and Soroban
transaction simulation.

## Why it exists

Stellar protocol upgrades can change XDR structures, RPC behavior, and
Soroban host semantics. Projects usually discover incompatibilities only
after upgrading a dependency or after a network has already moved to a new
protocol version. Protocol Canary lets a project rehearse that upgrade
locally and in CI, against real upstream Stellar types and a real RPC
endpoint, before the network moves.

## What it checks

- **XDR** — decode, encode, and round-trip fixtures against the official
  `stellar-xdr` crate.
- **RPC** — `getNetwork`, `getLatestLedger`, and related response-shape and
  protocol-identity assertions against a configured Stellar RPC endpoint.
- **Soroban** — transaction construction and `simulateTransaction` behavior
  for Soroban-specific protocol changes.

## Quick start

Install a released version from source:

```bash
cargo install --git https://github.com/StellarCanary/Protocol-Canary --tag v0.1.1 --locked
stellar-canary check
```

Or, from a local checkout of this repository:

```bash
cargo install --path crates/canary-cli
stellar-canary check
```

During development, run the CLI directly from the workspace:

```bash
cargo run -p canary-cli -- check
cargo run -p canary-cli -- inspect
cargo run -p canary-cli -- fixtures --protocol 28
cargo run -p canary-cli -- check --json > result.json && cargo run -p canary-cli -- report result.json --format markdown
```

## Example output

The following is illustrative example output, not a live result:

```text
Stellar Protocol Canary
──────────────────────────────

Protocol: 28
Project: example-soroban-app

XDR
  2/2 PASS

RPC
  2/2 PASS

Soroban
  3/3 PASS

──────────────────────────────

7/7 applicable checks passed.

Status: PASS
```

## Configuration

Protocol Canary reads `.stellar-canary.toml` from the project root:

```toml
version = 1
protocol = 28

[project]
type = "auto"

[tests]
xdr = true
rpc = true
soroban = true

[policy]
warnings_are_failures = false
```

## Protocol 28

The first compatibility pack targets Stellar Protocol 28: a real CAP-0083
XDR fixture, a live RPC network-identity fixture, and a real Soroban
simulation fixture, all verified against `soroban-testnet.stellar.org` and
included under `tests/fixtures/protocol-28/` for local development. See
[docs/protocol-28.md](docs/protocol-28.md) for exactly what is and is not
covered — CAP-0085 and CAP-0086 host-function semantics specifically are
intentionally left to the dedicated `ProtocolCanary-Fixtures` repository
rather than guessed at here.

## Ecosystem

This repository is CLI-only and has no GitHub dependency of its own. Two
sibling repositories build on it:

- [`ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures) —
  canonical, versioned compatibility fixtures (what gets tested).
- [`ProtocolCanary-Action`](https://github.com/StellarCanary/ProtocolCanary-Action) —
  the GitHub Actions integration that installs a pinned release of this
  CLI, runs it, and publishes a job summary, annotations, and a report
  artifact from its JSON output.

See [docs/architecture.md](docs/architecture.md) for how the three
repositories fit together, [docs/json-report-contract.md](docs/json-report-contract.md)
for the exact `--json` shape a consumer like the Action should parse, and
[docs/fixture-contract.md](docs/fixture-contract.md) for how fixtures are
supplied via `--fixtures-dir`.

## Version compatibility

| `Protocol-Canary` | Fixture pack | Target protocol | `ProtocolCanary-Action` |
|---|---|---|---|
| `v0.1.1` | `ProtocolCanary-Fixtures` `protocol-28/` | 28 | `v1` (default `version: "0.1.1"`) |

Only combinations that have actually been run together are listed here.
`v0.1.0` reports predate the `counts` field and predate `ContractExecutable`
XDR support (needed by two of the current Protocol 28 fixtures); the Action
tolerates the missing field but `v0.1.1` is the version this table
verifies against.

## Security

Protocol Canary never asks for or stores a secret key, seed phrase, or
private key, and the MVP never submits a transaction to any network — only
read operations and simulation are performed. See [SECURITY.md](SECURITY.md).

## Limitations

A passing result means the declared compatibility assertions for the
configured protocol passed. It is not a guarantee that an application
cannot break in some other way, and it does not replace testing against a
real testnet or mainnet deployment.

The local result cache (`canary_core::CacheStore`) exists and is tested,
but is not yet wired into `check`'s execution path — every run currently
calls RPC/Soroban fresh rather than reusing a prior result. See
[CHANGELOG.md](CHANGELOG.md) for the full known-gaps list.

## Roadmap

See [ROADMAP.md](ROADMAP.md) for planned work. Highlights: Protocol 29
support once its CAPs are finalized upstream, wiring the existing
`CacheStore` into `check`'s execution path, and additional RPC/Soroban
compatibility assertions.

## Workspace layout

See [CONTRIBUTING.md](CONTRIBUTING.md) for the crate layout and development
workflow.
