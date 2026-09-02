# Stellar Protocol Canary

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

```bash
cargo install --path crates/canary-cli
stellar-canary check
```

During development, run the CLI directly from the workspace:

```bash
cargo run -p canary-cli -- check
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

The first compatibility pack targets Stellar Protocol 28. It exercises the
following real protocol changes:

- **CAP-0083** — new `StellarValue` behavior used by validators to vote to
  drop a transaction set, tested at the XDR level.
- **CAP-0085** — externally managed contract executable references, tested
  through the Soroban simulation surface.
- **CAP-0086** — sparse-map host functions for efficient migration, tested
  through the Soroban simulation surface.

Fixture data for these checks lives in the separate `ProtocolCanary-Fixtures`
repository; see [docs/protocol-28.md](docs/protocol-28.md) once fixtures are
integrated.

## CI

This repository is CLI-only and has no GitHub dependency. A separate
repository, `ProtocolCanary-Action`, wraps this CLI as a GitHub Action and
maps its exit codes to CI annotations and job summaries.

## Security

Protocol Canary never asks for or stores a secret key, seed phrase, or
private key, and the MVP never submits a transaction to any network — only
read operations and simulation are performed. See [SECURITY.md](SECURITY.md).

## Limitations

A passing result means the declared compatibility assertions for the
configured protocol passed. It is not a guarantee that an application
cannot break in some other way, and it does not replace testing against a
real testnet or mainnet deployment.

## Workspace layout

See [CONTRIBUTING.md](CONTRIBUTING.md) for the crate layout and development
workflow.
