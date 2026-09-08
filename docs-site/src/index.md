# Protocol Canary

Protocol Canary is a command-line tool that checks whether a Stellar
application or piece of infrastructure remains compatible with a target
Stellar protocol version. It runs a set of explicit, declared compatibility
assertions ("fixtures") against three surfaces a Stellar project typically
depends on: **XDR** encoding/decoding, **Stellar RPC** responses, and
**Soroban** transaction simulation.

It is developer infrastructure — a CLI and a GitHub Action, not a frontend
application, not a smart contract, not a wallet, and not a hosted service.
It does not certify that an arbitrary application is completely compatible
with a Stellar protocol version; it reports whether the compatibility
assertions currently implemented in its fixture packs pass against your
configured dependencies and RPC endpoint.

## What it checks

| Surface | What it verifies |
|---|---|
| XDR | Decode, encode, and round-trip fixtures against the official `stellar-xdr` crate. Runs fully offline — no network call. |
| RPC | `getNetwork`/`getLatestLedger` response-shape and protocol-identity assertions against a configured Stellar RPC endpoint. |
| Soroban | Unsigned transaction construction and `simulateTransaction` behavior. Never submits a transaction and never requires a private key. |

## Who this is for

- **New to Protocol Canary?** Start with [What Problem Does It Solve?](./problem.md) and [How It Works](./how-it-works.md).
- **Evaluating Protocol 28 compatibility?** Go straight to [Protocol 28](./protocol-28.md).
- **Wiring this into CI?** See the [GitHub Action](./github-action.md) guide.
- **Adding a compatibility fixture or engine check?** See [Fixtures](./fixtures-guide.md) and [Authoring a Fixture](./fixture-authoring.md).
- **Reviewing this project?** [Architecture](./architecture.md), [Security](./security.md), and [Limitations](./limitations.md) are the three pages written specifically for that.

## Quick example

```bash
cargo install --git https://github.com/StellarCanary/Protocol-Canary --tag v0.1.1 --locked
stellar-canary check --fixtures-dir <checkout-of-ProtocolCanary-Fixtures>/protocol-28 --protocol 28
```

```text
Stellar Protocol Canary
────────────────────────────────────────

Project: Protocol-Canary (unknown)
Target protocol: 28
Network: testnet (observed protocol 28)

XDR
  3/3 PASS

RPC
  1/1 PASS

Soroban
  1/1 PASS

────────────────────────────────────────

5/5 applicable checks passed.

Status: PASS
```

This is a real run's output (not illustrative text) — see
[Your First Check](./first-check.md) to reproduce it yourself.

## Current support

| | |
|---|---|
| CLI release | `Protocol-Canary` [`v0.1.1`](./releases.md) |
| GitHub Action | `ProtocolCanary-Action` [`v1`](./github-action.md) (currently resolves to `v0.1.1`) |
| Fixture pack | [`ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures) `protocol-28/` — 5 currently implemented checks |
| Protocol coverage | [Protocol 28](./protocol-28.md) only. CAP-0086 is a documented gap, not silently skipped. |

## Ecosystem

Three repositories make up Protocol Canary:

| Repository | Purpose |
|---|---|
| [`StellarCanary/Protocol-Canary`](https://github.com/StellarCanary/Protocol-Canary) | The compatibility engine — the `stellar-canary` CLI. |
| [`StellarCanary/ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures) | Canonical, versioned compatibility fixtures (declarative TOML data, no executable code). |
| [`StellarCanary/ProtocolCanary-Action`](https://github.com/StellarCanary/ProtocolCanary-Action) | The GitHub Actions integration that installs a pinned CLI release and turns its report into a job summary, annotations, and an artifact. |

See [Architecture](./architecture.md) for how they fit together, [CLI
Reference](./cli/overview.md) for every command, and
[Contributing](./contributing.md) to work on any of the three.
