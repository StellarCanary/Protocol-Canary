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

## Execution topology

Protocol Canary runs in exactly two places, plus one external network it
talks to. There is no other component — no hosted backend, no database, no
persistent server.

```text
LOCAL (developer machine)                  EXTERNAL (public network)
--------------------------                 --------------------------
Developer
  |
  v
stellar-canary check  ------------------->  Stellar Testnet RPC
  |         |                                 |  getNetwork
  |         `--------------------------->      `  simulateTransaction
  |                                           (read-only / simulation-only,
  v                                            no submission, no key)
fixture directory
(local path, or a
 ProtocolCanary-Fixtures checkout)
  |
  v
report (terminal / JSON / Markdown)


CI (GitHub-hosted, ephemeral per run)
--------------------------------------
Developer's repository
  |
  v
GitHub Actions workflow
  |
  v
StellarCanary/ProtocolCanary-Action@v1   (uses: StellarCanary/ProtocolCanary-Action@v1
  |                                        in a consumer's workflow — not uses: ./,
  |                                        which only appears inside this Action's
  |                                        own internal integration.yml)
  v
cargo install --git Protocol-Canary      -> same "LOCAL" flow above, run
  |                                        inside the runner's ephemeral VM
  v
JSON report
  |
  v
GitHub job summary + annotations + workflow artifact
```

Both the local and CI flows run the identical `stellar-canary` binary
against the identical fixture format — the Action does not reimplement or
reinterpret anything; see [Trust boundaries](#trust-boundaries).

## Local execution path

```text
Developer
  |
  v
stellar-canary check --fixtures-dir <dir>
  |
  v
fixture directory (loaded + validated)
  |
  v
compatibility runner
  ├── XDR       (offline — decodes/encodes against the stellar-xdr crate,
  |              no network call)
  ├── RPC       (requires a live RPC endpoint — getNetwork)
  └── Soroban   (requires a live RPC endpoint — simulateTransaction)
  |
  v
report: terminal (default) or --format json / markdown
```

Only the XDR surface is offline. A run with `[tests] rpc = true` or
`soroban = true` (the default, and the case for the `protocol-28` pack)
requires network access to the configured `--rpc-url` — this project does
**not** claim the tool works with zero network access; it claims the tool
never requires a **private key** or **transaction submission**, which is a
different guarantee. Disabling `rpc`/`soroban` in `.stellar-canary.toml`
(or using an XDR-only fixture set) is the documented way to run fully
offline; see [Step 10 — network failure](#network-failure-behavior) for
what happens when RPC/Soroban are enabled but the endpoint is unreachable.

## Live network topology

```text
Protocol-Canary (canary-rpc)  ---getNetwork-------->  Stellar Testnet RPC
Protocol-Canary (canary-soroban) --simulateTransaction--> Soroban environment
```

- `simulateTransaction` simulates only — it never submits, and Soroban
  simulation cannot itself mutate ledger state.
- No private key or seed phrase is read, stored, or transmitted anywhere
  in the codebase (verified: the only `std::env` calls in
  `Protocol-Canary` are `temp_dir()`/`current_dir()`; there is no key
  material path).
- Live checks make outbound read/simulate calls only; nothing in a normal
  `check` run mutates the target network.
- The overall tool is **not** "offline" — RPC/Soroban surfaces genuinely
  require the configured endpoint to be reachable, and a run that enables
  them will fail (not silently skip) if it is not; see below.

## Network failure behavior

Verified directly (Phase 8 audit, reproduced 2026-09-03) by pointing
`--rpc-url` at an unreachable host with the real `protocol-28` fixtures:

| Aspect | Observed behavior |
|---|---|
| Exit code | `3` (`ExecutionError`) |
| `status` | `"error"` (overrides what pass/fail counts alone would suggest) |
| XDR checks | Still run and pass — offline surfaces are unaffected by an RPC outage |
| `network` object | Present (`name` only) — `observedProtocol` is correctly **absent**, since the live call never returned a value |
| `results[].status` for the affected fixtures | `"error"`, with a real transport error message (e.g. `network transport error calling getNetwork: error sending request for url (...)`, not a fabricated one) |
| Report structure | Remains fully valid JSON matching the same schema as a pass/fail run — a consumer does not need special-case parsing for the offline case |

No retry or fallback logic exists or was added — a single failed call is
reported as an error for that fixture; the run does not hang or crash.

## GitHub Action topology

An external consumer's workflow references the released Action by tag:

```yaml
- uses: actions/checkout@v4
- uses: StellarCanary/ProtocolCanary-Action@v1
  with:
    protocol: "28"
```

`v1` is a floating tag that currently resolves to commit `4279000` (the
same commit as the `v0.1.1` release tag) — verified via the GitHub API,
and independently verified end-to-end by pushing a disposable test
repository that used exactly this `@v1` reference (not the Action's own
internal `uses: ./`) against the real `ProtocolCanary-Fixtures`
`protocol-28` pack: PASS run
[33808765160](https://github.com/Hollujay/protocol-canary-phase8-smoketest/actions/runs/33808765160)
(5/5, `status: pass`) and a deliberate FAIL run
[33809326503](https://github.com/Hollujay/protocol-canary-phase8-smoketest/actions/runs/33809326503)
(job failed, annotation named the exact failing fixture, `status: fail`).
Both produced the `stellar-protocol-canary-report` JSON artifact matching
the documented schema.

## Fixture topology

`ProtocolCanary-Fixtures` supplies **declarative data only** (TOML) — it
is never executed as code by any component. `Protocol-Canary` is the only
component that interprets fixture content (`canary-fixtures` loads it,
`canary-xdr`/`canary-rpc`/`canary-soroban` execute the assertion it
describes). `ProtocolCanary-Action` never reads a fixture file itself; it
only passes `--fixtures-dir` through to the CLI and parses the CLI's JSON
output. This division is intentional — see
[Trust boundaries](#trust-boundaries).

## Version dependency graph

```text
Protocol-Canary v0.1.1  ────requires (for the current pack)───▶  ProtocolCanary-Fixtures protocol-28

ProtocolCanary-Action v0.1.1 / v1  ────installs & pins───▶  Protocol-Canary v0.1.1
```

This is the only combination that has actually been run end-to-end and
verified (see the main [README's version compatibility
table](../README.md#version-compatibility)). `v0.1.0` is documented as
installable but predates `ContractExecutable` XDR support (needed by two
current `protocol-28` fixtures) and the `counts` field — it is not a
verified combination for the current fixture pack. No future protocol
version (29+) is supported by any released version; do not infer support
from this document.

## Stateful vs. ephemeral components

| Component | State model |
|---|---|
| `Protocol-Canary` (`stellar-canary` binary) | Stateless per invocation. The `canary_core::CacheStore` type exists and is unit-tested but is **not** wired into `check`'s execution path (see `CHANGELOG.md`'s "Known gaps") — every run calls RPC/Soroban fresh. |
| `ProtocolCanary-Fixtures` | State lives only as version-controlled git history — a fixture pack is a fixed snapshot at a given commit, not a database. |
| `ProtocolCanary-Action` | Fully ephemeral — runs inside a GitHub-hosted runner VM that is destroyed after the job. Its only durable output is the workflow artifact/job summary GitHub stores. `actions/cache` may cache a built `stellar-canary` binary between runs on the same repo as a best-effort speedup; this is never required for correctness. |
| Stellar RPC / Testnet | External network state, owned and operated by the Stellar network, outside this project entirely. Protocol Canary only reads from it (`getNetwork`) or simulates against it (`simulateTransaction`) — it never writes to it. |

No component in this system introduces its own persistent application
storage (no database, no hosted API state).

## Failure boundaries

| Failure | Behavior |
|---|---|
| RPC endpoint unreachable | Exit `3`, `status: "error"`, offline (XDR) checks still run; see [Network failure behavior](#network-failure-behavior). |
| Malformed fixture file | Exit `4` (`InvalidFixture`); the CLI reports which file and the parse error, and does not run any fixtures from that load. |
| A fixture's compatibility assertion fails | Exit `1`, `status: "fail"`; the specific failing `testId` and detail are reported in both terminal and JSON. |
| Invalid CLI configuration (e.g. `--config` path does not exist) | Exit `2` (`ConfigurationError`) with a specific message; no checks run. |
| The Action cannot install or execute Canary at all (build/timeout/unparseable output) | Action's own `status` output is `"execution-failed"` (distinct from a `fail` compatibility result) — the summary states "Protocol Canary could not be executed" with the real diagnostic, never a fabricated compatibility message. |
| A report is missing the `counts` field (older `schemaVersion`-1 output) | The Action derives `counts` from `results` rather than erroring — see `ProtocolCanary-Action`'s `tests/unit/output.test.ts`. Backward compatibility is preserved, not removed. |
| GitHub artifact upload fails | Logged as a warning; the underlying compatibility result and job pass/fail are unaffected — artifact upload is auxiliary, never load-bearing for the result. |
| Job summary itself fails to publish | Reported distinctly, as "Failed to publish Canary summary" — never conflated with either an execution or compatibility failure. |

## Environment variables

| Name | Repository / component | Required? | Default | Purpose | Sensitive? |
|---|---|---|---|---|---|
| *(none)* | `Protocol-Canary` | — | — | The CLI reads no named configuration environment variable. It uses only OS-level `current_dir()` (project root detection) and `temp_dir()` (scratch files in tests/cache code) — no `.env`, no secret, no required variable of any kind. | No |
| *(none)* | `ProtocolCanary-Fixtures` | — | — | The Python validator and test suite use only the standard library; no environment variable is read. | No |
| `CARGO_HOME` | `ProtocolCanary-Action` | Optional | `~/.cargo` (via `os.homedir()/.cargo`) | Where the Action looks for/installs the `cargo`-built `stellar-canary` binary. | No |
| `RUNNER_TEMP` | `ProtocolCanary-Action` | Optional (GitHub-provided on Actions runners) | `os.tmpdir()` | Scratch directory for the installed binary and generated report. | No |
| `GITHUB_TOKEN` | `ProtocolCanary-Action` (`release.yml` only) | Auto-provided by GitHub Actions | — | Used implicitly by `softprops/action-gh-release@v2` to publish a GitHub Release under `contents: write`. Only relevant to the Action's own maintainers cutting a release — a consumer of the Action never sets or sees this. | Yes (but managed entirely by GitHub, never configured by a user) |

No component in this system requires a Stellar secret key, seed phrase,
or any other credential, confirming the claim in each repository's
`SECURITY.md`.

## External dependency table

| Dependency | Used by | When |
|---|---|---|
| Rust toolchain (pinned `1.91.0`, `rust-toolchain.toml`) | `Protocol-Canary` | Build-time (compiling the CLI, whether locally or inside the Action's runner) |
| crates.io | `Protocol-Canary` | Build-time only (dependency resolution/download; nothing at runtime) |
| GitHub (repository hosting, releases, tags) | All three repos | Distribution — `cargo install --git`, Action `@v1` resolution, fixture checkout |
| GitHub Actions (runner infrastructure) | `ProtocolCanary-Action` | CI-only — the Action does not run outside GitHub Actions |
| Stellar RPC (Testnet by default, or mainnet/futurenet/custom if configured) | `Protocol-Canary` (`canary-rpc`, `canary-soroban`) | Runtime, network-only — only when `rpc`/`soroban` checks are enabled |
| Python 3 (stdlib only, no `requirements.txt`) | `ProtocolCanary-Fixtures` | Development/CI-only, for `tools/validate/validate.py` and its test suite |
| Node.js (`>=20` declared; CI pins `24`) | `ProtocolCanary-Action` | Build-time and runtime (the Action itself is a Node action) |
| npm dependencies (`package-lock.json`, incl. `@actions/*`, `@octokit/*`) | `ProtocolCanary-Action` | Build-time / bundled into the committed `dist/index.js` at runtime |

## Cost / hosting model

- No paid backend, persistent server, or database is required for any
  normal use of `Protocol-Canary`, `ProtocolCanary-Fixtures`, or
  `ProtocolCanary-Action`.
- No private key or seed phrase is ever required.
- No Docker installation is required for normal CLI or Action use (both
  install and run directly on a developer machine or a standard
  GitHub-hosted Ubuntu runner).
- GitHub-hosted Actions runners are used for CI execution — free-tier
  GitHub Actions minutes are sufficient for the workflows in this
  ecosystem; nothing requires a paid GitHub tier.
- Live compatibility checks contact the public Stellar network (Testnet
  by default) exactly the way any Stellar client would — this is public
  infrastructure access, not a paid or private backend operated by this
  project.

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
