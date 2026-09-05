# GitHub Action

[`StellarCanary/ProtocolCanary-Action`](https://github.com/StellarCanary/ProtocolCanary-Action)
is the official GitHub Actions integration. It is a thin wrapper around
the real `stellar-canary` CLI — it does not know what a CAP means, does
not reimplement XDR/RPC/Soroban testing, and never reinterprets a
compatibility result the CLI didn't report.

## What it does

1. Installs the requested `Protocol-Canary` version (from source, pinned
   to an immutable commit — see [Installation & integrity](#installation--integrity)).
2. Runs `stellar-canary check --format json` with your inputs.
3. Publishes a GitHub job summary and (optionally) annotations from that
   JSON.
4. Optionally uploads the JSON report as a workflow artifact.
5. Passes or fails the job according to Canary's own exit code — never a
   result the Action computed itself.

## What it does not do

- It does not reimplement any compatibility check itself.
- It does not require a private key, and never submits a transaction —
  same guarantee as the CLI it wraps.
- It does not use Docker, a paid service, or a hosted backend.
- There is deliberately no `format` input: the Action always requests
  `--format json` (the only way it can build the summary and
  annotations), and never invokes Canary twice.

## Quick start

```yaml
- uses: actions/checkout@v4
- uses: StellarCanary/ProtocolCanary-Action@v1
  with:
    protocol: "28"
```

`@v1` is the published, floating major-version tag — the documented
consumer path. (`uses: ./` only appears inside this Action's own internal
integration test workflow; it is not something a consumer should write.)

## Inputs

| Input | Description | Default |
|---|---|---|
| `protocol` | Target Stellar protocol version (`--protocol`). | (from `.stellar-canary.toml`, or 28) |
| `config` | Path to `.stellar-canary.toml` (`--config`). Fails clearly if the given path does not exist. | (CLI default lookup) |
| `network` | Network for live RPC/Soroban checks (`--network`). | `testnet` (CLI default) |
| `rpc-url` | Stellar RPC endpoint (`--rpc-url`). Must be `https://`, or `http://localhost`/`127.0.0.1` for local development. | (none) |
| `fixtures-dir` | Path to a directory of fixtures, e.g. a checkout of `ProtocolCanary-Fixtures` (`--fixtures-dir`). | `fixtures` |
| `version` | `Protocol-Canary` version to install, without a leading `v`. Pinned — never tracks `main`. | `0.1.1` |
| `upload-report` | Upload the JSON report as a workflow artifact. | `true` |
| `annotations` | Emit GitHub annotations for failures/warnings/errors. | `true` |
| `timeout-minutes` | Maximum time to let Canary run before it is terminated. | `15` |

## Outputs

| Output | Description |
|---|---|
| `status` | `pass`, `warning`, `fail`, `error`, or `execution-failed` (the Action's own value when Canary could not produce a report at all). |
| `passed` | Number of checks that passed. |
| `warnings` | Number of checks that produced a warning. |
| `failures` | Number of checks that failed a compatibility assertion. |
| `errors` | Number of checks that could not complete due to an execution error. |
| `report` | Absolute path to the generated JSON report file. |

## Fixtures checkout

The Action does not read fixture files itself — it only forwards
`--fixtures-dir` to the CLI. To check against the real Protocol 28 pack,
check it out as a separate step and point `fixtures-dir` at it:

```yaml
- uses: actions/checkout@v4
- uses: actions/checkout@v4
  with:
    repository: StellarCanary/ProtocolCanary-Fixtures
    path: canary-fixtures
- uses: StellarCanary/ProtocolCanary-Action@v1
  with:
    protocol: "28"
    fixtures-dir: canary-fixtures/protocol-28
    network: testnet
    rpc-url: https://soroban-testnet.stellar.org
```

## Installation & integrity

`Protocol-Canary` does not yet publish prebuilt release binaries or
checksums (see [Releases](./releases.md)). This Action installs it with
`cargo install --git`, pinned to the immutable commit the requested
version's tag resolved to at run time (falling back to the tag itself,
with a visible warning, only if that resolution fails). This requires a
Rust/Cargo toolchain on the runner — GitHub-hosted Ubuntu runners include
one by default. A successful build is cached (best-effort, via
`actions/cache`; never required for correctness).

## Summary, annotations, and artifact

- **Job summary** — a per-surface pass/fail table rendered from the JSON
  report.
- **Annotations** (when `annotations: true`, the default) — each failing,
  warning, or errored fixture becomes a GitHub Actions annotation pointing
  at the specific `testId`.
- **Artifact** (when `upload-report: true`, the default) — the raw JSON
  report is uploaded as a workflow artifact named
  `stellar-protocol-canary-report`. Artifact upload is always auxiliary:
  if it fails, the underlying compatibility result is unaffected, and a
  warning is logged rather than the job failing on that account alone.

## Failure behavior

See [CI Workflows & Failure Behavior](./ci-workflows.md) for the full
distinction between a compatibility failure and an execution failure, and
what each looks like in a real workflow run.

## Timeout behavior

`timeout-minutes` (default `15`) bounds how long the Canary process is
allowed to run. If it is exceeded, the process is terminated and the
Action reports `status: execution-failed` with a timeout diagnostic —
never a fabricated compatibility result.

## Supported Canary versions

| Action | Protocol-Canary |
|---|---|
| `v1` | `0.1.1` (default; `0.1.0` also installable, but predates the `ContractExecutable` XDR type two current Protocol 28 fixtures require, and its report predates the `counts` field) |

This table grows as `Protocol-Canary` cuts new releases; a `schemaVersion`
change to its JSON report would be a breaking change for this Action and
would be called out here explicitly.
