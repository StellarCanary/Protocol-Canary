# CI Workflows & Failure Behavior

## A complete, minimal workflow

This is a real workflow, taken directly from
[`ProtocolCanary-Action`'s `examples/protocol-28.yml`](https://github.com/StellarCanary/ProtocolCanary-Action/blob/main/examples/protocol-28.yml)
— checking a project against the real Protocol 28 fixture pack, against a
live Testnet RPC endpoint:

```yaml
name: Stellar Compatibility (Protocol 28)

on:
  pull_request:

permissions:
  contents: read

jobs:
  compatibility:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout project
        uses: actions/checkout@v4

      - name: Checkout Protocol 28 fixtures
        uses: actions/checkout@v4
        with:
          repository: StellarCanary/ProtocolCanary-Fixtures
          path: canary-fixtures

      - name: Stellar Protocol Canary
        uses: StellarCanary/ProtocolCanary-Action@v1
        with:
          protocol: "28"
          fixtures-dir: canary-fixtures/protocol-28
          network: testnet
          rpc-url: https://soroban-testnet.stellar.org
```

`permissions: contents: read` is sufficient — the Action does not need
write access to your repository to check compatibility. A project with no
Soroban contracts, for example, can set `[tests] soroban = false` in its
own `.stellar-canary.toml` without this workflow needing to change.

## Using the Action's outputs

```yaml
- name: Stellar Protocol Canary
  id: canary
  uses: StellarCanary/ProtocolCanary-Action@v1
  with:
    protocol: "28"
    upload-report: "true"

- name: Show result
  if: always()
  run: |
    echo "status: ${{ steps.canary.outputs.status }}"
    echo "passed: ${{ steps.canary.outputs.passed }}"
    echo "failures: ${{ steps.canary.outputs.failures }}"
```

(From [`examples/pull-request.yml`](https://github.com/StellarCanary/ProtocolCanary-Action/blob/main/examples/pull-request.yml).)

## Two different kinds of "red"

The Action distinguishes a **compatibility failure** from an **execution
failure** — they are not the same thing, and it never conflates them:

| | Compatibility failure | Execution failure |
|---|---|---|
| Meaning | Canary ran successfully and found a real incompatibility | Canary could not be installed, could not run, timed out, or produced output that could not be parsed |
| `status` output | `fail` (or `warning`/`error`) | `execution-failed` |
| Job summary | A per-surface table naming the specific failing test IDs | "Protocol Canary could not be executed" with the actual diagnostic — never a fabricated compatibility message |

A third, separate case — the job summary itself failing to publish — is
reported as "Failed to publish Canary summary," distinct from both.

## What a compatibility failure looks like

When a fixture's assertion genuinely fails, the underlying CLI exits `1`
(see [Exit Codes](./exit-codes.md)), and:

- The **job fails** — the workflow step reports non-zero.
- The **job summary** shows the per-surface pass/fail table with the
  specific failing `testId`(s).
- A **GitHub annotation** is created pointing at the failing fixture (when
  `annotations: true`, the default).
- The **artifact** (`stellar-protocol-canary-report`, when
  `upload-report: true`) still uploads — a compatibility failure is a
  real result, not an upload problem, so the report is preserved for
  inspection either way.

This has been verified end-to-end against the real, published `@v1`
Action: a deliberate FAIL run produced a failed job, an annotation naming
the exact failing fixture, `status: fail`, and the JSON artifact matching
the documented schema — the same pipeline as a PASS run, just with a
different, honestly reported result.

## Network vs. compatibility failures

A failure to reach the configured RPC endpoint is **not** a compatibility
failure — it is an execution problem. `status` is `error` (not `fail`),
the affected fixtures report `"status": "error"` with a transport-error
message, and any offline (XDR) fixtures in the same run are unaffected and
still report their real pass/fail result. See
[Network Troubleshooting](./troubleshooting.md#network-troubleshooting)
for how to tell the two apart when triaging a red CI job.
