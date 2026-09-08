# Architecture

This page summarizes the verified architecture. The full, exhaustive
version — environment variables, external-dependency tables, and the
complete failure-boundary matrix — lives in
[`Protocol-Canary`'s `docs/architecture.md`](https://github.com/StellarCanary/Protocol-Canary/blob/main/docs/architecture.md)
and is authoritative; this page reproduces its key diagrams rather than
duplicating it in full, per this site's own
[documentation change discipline](./contributing.md).

## Component responsibilities

| Repository | Responsibility |
|---|---|
| [`Protocol-Canary`](https://github.com/StellarCanary/Protocol-Canary) | The compatibility engine: the `stellar-canary` CLI, project detection, fixture loading/validation, the XDR/RPC/Soroban runners, policy evaluation, and terminal/JSON/Markdown reporting. |
| [`ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures) | Canonical, versioned compatibility assertions ("fixtures") for each protocol version. Declarative TOML data only — no business logic, no executable code. |
| [`ProtocolCanary-Action`](https://github.com/StellarCanary/ProtocolCanary-Action) | GitHub Actions integration. A thin wrapper that installs a pinned `Protocol-Canary` release, runs it, and turns its JSON report into a job summary, annotations, and an artifact. |

`Protocol-Canary` depends on nothing outside itself to run. `ProtocolCanary-Action`
depends on a released `Protocol-Canary` version and, in practice, a
`ProtocolCanary-Fixtures` checkout. Neither `Protocol-Canary` nor
`ProtocolCanary-Fixtures` depends on the Action — both work standalone
from the command line.

## Execution topology

Protocol Canary runs in exactly two places, plus one external network it
talks to. There is no hosted backend and no database.

```text
LOCAL (developer machine)                  EXTERNAL (public network)
--------------------------                 --------------------------
Developer
  │
  ▼
stellar-canary check  ------------------->  Stellar RPC (Testnet by default)
  │         │                                 │  getNetwork
  │         `--------------------------->      `  simulateTransaction
  │                                           (read-only / simulation-only,
  ▼                                            no submission, no key)
fixture directory
(local path, or a
 ProtocolCanary-Fixtures checkout)
  │
  ▼
report (terminal / JSON / Markdown)


CI (GitHub-hosted, ephemeral per run)
--------------------------------------
Developer's repository
  │
  ▼
GitHub Actions workflow
  │
  ▼
StellarCanary/ProtocolCanary-Action@v1
  │
  ▼
cargo install --git Protocol-Canary   -> same LOCAL flow above, run
  │                                      inside the runner's ephemeral VM
  ▼
JSON report
  │
  ▼
GitHub job summary + annotations + workflow artifact
```

Both the local and CI flows run the identical `stellar-canary` binary
against the identical fixture format — the Action never reimplements or
reinterprets a compatibility result; it passes or fails the job according
to the CLI's own exit code and `status` field.

## Trust boundaries

- **Fixtures are untrusted data, not code.** A fixture file is declarative
  TOML; no field executes a shell command, script, or arbitrary code.
- **No private keys.** Nothing in this system ever reads, stores, or
  transmits a private key or seed phrase. The Soroban surface builds and
  simulates unsigned transactions only.
- **No transaction submission.** Every network interaction is read-only
  (`getNetwork`, `getLatestLedger`) or simulation-only
  (`simulateTransaction`).
- **The Action never reinterprets a result.** It parses the CLI's JSON and
  passes or fails the job according to the CLI's own exit code / `status`
  field.

## Stateful vs. ephemeral components

| Component | State model |
|---|---|
| `stellar-canary` binary | Stateless per invocation. A local result cache (`canary_core::CacheStore`) exists and is unit-tested but is **not yet wired into `check`'s execution path** — every run calls RPC/Soroban fresh. See [Limitations](./limitations.md). |
| `ProtocolCanary-Fixtures` | State lives only as version-controlled git history — a fixture pack is a fixed snapshot at a given commit. |
| `ProtocolCanary-Action` | Fully ephemeral — runs inside a GitHub-hosted runner VM destroyed after the job. Its only durable output is the workflow artifact/job summary GitHub stores. |
| Stellar RPC / Testnet | External network state, owned by the Stellar network, outside this project. Protocol Canary only reads from it or simulates against it — never writes. |

No component in this system introduces its own persistent application
storage: no database, no hosted API state, no server.

## Cost / hosting model

No paid backend, persistent server, or database is required for any
normal use of any of the three repositories. No Docker installation is
required. GitHub-hosted Actions runners on the free tier are sufficient
for the CI workflows in this ecosystem.

For the complete picture — including the environment-variable table, the
external-dependency table, and every documented failure boundary — see
[`docs/architecture.md`](https://github.com/StellarCanary/Protocol-Canary/blob/main/docs/architecture.md)
in the `Protocol-Canary` repository.
