# JSON report contract

This documents the actual `stellar-canary check --json` (equivalently
`--format json`) output, as implemented in `crates/canary-report/src/json.rs`,
and verified by running the built CLI against the real fixtures in
`tests/fixtures/protocol-28/`. This is the contract
[`ProtocolCanary-Action`](https://github.com/StellarCanary/ProtocolCanary-Action)
consumes to build its GitHub job summary and annotations.

## Command

```bash
stellar-canary check --protocol 28 --json
# equivalently:
stellar-canary check --protocol 28 --format json
```

`--protocol` is optional (defaults to `.stellar-canary.toml`'s `protocol`
field, or 28 if there is no config file). Exit code follows the
documented contract (0 pass, 1 compatibility failure, 2 configuration
error, 3 execution/RPC error, 4 invalid fixture, 5 internal error) —
**the JSON is printed to stdout regardless of exit code**, including on
a compatibility failure.

## Top-level shape

```json
{
  "schemaVersion": 1,
  "toolVersion": "0.1.1",
  "targetProtocol": 28,
  "project": {
    "name": "example-project",
    "type": "soroban"
  },
  "network": {
    "name": "testnet",
    "observedProtocol": 28
  },
  "status": "pass",
  "counts": {
    "total": 3,
    "passed": 3,
    "failed": 0,
    "warnings": 0,
    "errors": 0,
    "skipped": 0
  },
  "results": [
    {
      "testId": "p28-xdr-cap83-empty-tx-set",
      "protocol": 28,
      "surface": "xdr",
      "status": "pass",
      "summary": "StellarValue round-tripped byte-for-byte",
      "durationMs": 1,
      "fixtureId": "p28-xdr-cap83-empty-tx-set"
    }
  ],
  "skipped": [],
  "git": {
    "commit": "68a5f8d52573fa741e095bc321443015f0f21250",
    "branch": "main",
    "isDirty": false
  }
}
```

### Field reference

| Field | Type | Always present? | Notes |
|---|---|---|---|
| `schemaVersion` | integer | yes | Currently `1`. An incompatible change to this shape bumps this; purely additive fields (like `git` and `counts` were) do not. |
| `toolVersion` | string | yes | The Canary CLI's own semver (`CARGO_PKG_VERSION`), independent of `schemaVersion` and `targetProtocol`. |
| `targetProtocol` | integer | yes | The protocol this run checked against. |
| `project.name` | string | yes | Directory name of the project root. |
| `project.type` | string | yes | One of `"soroban"`, `"rpc-consumer"`, `"stellar-sdk"`, `"generic-stellar"`, `"unknown"`. |
| `network` | object \| absent | only when `[tests].rpc` or `[tests].soroban` is enabled | Omitted entirely for a fully offline (XDR-only) run — its absence *is* the offline signal; do not treat a missing `network` as an error. |
| `network.name` | string | when `network` present | `"testnet"`, `"mainnet"`, `"futurenet"`, or a custom network name. |
| `network.observedProtocol` | integer \| absent | when `network` present and the `getNetwork` call succeeded | Absent if the live network call failed — compare against `targetProtocol` to detect a protocol mismatch; do not assume they match. |
| `status` | string | yes | The single overall outcome: `"pass"`, `"warning"`, `"fail"`, or `"error"`. `"error"` means at least one result is `"error"` (an execution problem, e.g. an RPC timeout) and **overrides** what the pass/warning/fail policy decision would otherwise have been — this is the same precedence rule the process exit code follows. |
| `counts` | object | yes | Aggregate counts over `results` only (`skipped` fixtures are counted separately in `counts.skipped` and are never included in `counts.total`). Purely a convenience — always re-derivable from `results[].status`. |
| `counts.total` | integer | yes | `results.length`. |
| `counts.passed` / `.failed` / `.warnings` / `.errors` | integer | yes | Count of each `Status` value among `results`. |
| `counts.skipped` | integer | yes | `skipped.length`. |
| `results` | array | yes | One entry per fixture that actually ran (never includes skipped fixtures). Empty array is valid (e.g. no fixtures found) and is a pass, not an error. |
| `results[].testId` / `.fixtureId` | string | yes (`fixtureId` may be `null`) | Currently always equal to each other and to the fixture's `id`; treat them as the same identifier. |
| `results[].protocol` | integer | yes | The fixture's own declared protocol (matches `targetProtocol`, since non-matching fixtures are skipped before this point). |
| `results[].surface` | string | yes | `"xdr"`, `"rpc"`, or `"soroban"` (always lowercase — contrast with the capitalized headings in the terminal/Markdown reporters). |
| `results[].status` | string | yes | `"pass"`, `"warning"`, `"fail"`, `"error"`, or `"skipped"` (in practice `results[]` entries are never `"skipped"` — see above). |
| `results[].summary` | string | yes | One-line human-readable outcome. |
| `results[].details` | string \| absent | only when there is something to say | Present for failures/errors (and some passes); a multi-line explanation. Absent, not `null`, when there is nothing further to add. |
| `results[].durationMs` | integer | yes | Wall-clock time for that one fixture. |
| `skipped` | array | only when non-empty | Fixtures the planner decided not to run, and why. Omitted entirely (not `[]`) when there are none. |
| `skipped[].fixtureId` | string | yes | |
| `skipped[].surface` | string | yes | |
| `skipped[].reason` | string | yes | Human-readable, e.g. `"fixture targets protocol 27, this run targets protocol 28"` or `"rpc checks are disabled in configuration"`. |
| `git` | object | yes | Always present, with `null` fields when unavailable (not a Git repository, detached HEAD, etc.) — never omitted, never an error. |
| `git.commit` / `.branch` | string \| null | yes | |
| `git.isDirty` | boolean \| null | yes | |

### What a downstream consumer should key off of

- **Overall pass/fail**: `status` (not the process exit code, if you're
  parsing a saved file rather than checking `$?` directly) — remember
  `"error"` takes precedence over what `counts` alone would suggest.
- **Per-check detail for a PR comment or annotation**: iterate `results`,
  using `surface` to group and `status`/`summary`/`details` for content.
- **"N/M passed" style summaries**: `counts.passed` / `counts.total`
  directly — never compute a percentage without stating the denominator
  (this project's own reporters deliberately never do).
- **Protocol mismatch detection**: compare `targetProtocol` to
  `network.observedProtocol` when both are present.

### Verified failure-case example

Running against a deliberately malformed fixture (see
`crates/canary-cli/tests/check_offline.rs`'s
`a_real_compatibility_failure_is_reported_consistently_in_terminal_and_json`
regression test) produces, with exit code `1`:

```json
{
  "schemaVersion": 1,
  "toolVersion": "0.1.1",
  "targetProtocol": 28,
  "project": { "name": "check-fail-json", "type": "unknown" },
  "status": "fail",
  "counts": { "total": 1, "passed": 0, "failed": 1, "warnings": 0, "errors": 0, "skipped": 0 },
  "results": [
    {
      "testId": "p28-xdr-regression-fail",
      "protocol": 28,
      "surface": "xdr",
      "status": "fail",
      "summary": "failed to decode StellarValue",
      "details": "Invalid symbol 45, offset 3.",
      "durationMs": 0,
      "fixtureId": "p28-xdr-regression-fail"
    }
  ],
  "git": { "commit": null, "branch": null, "isDirty": false }
}
```

(`network` is absent here because the offline test config disables `rpc`
and `soroban`.)

## Reading a saved report back

`stellar-canary report <path.json> --format <terminal|json|markdown>`
parses exactly this shape (`JsonReporter::parse`) and re-renders it
without touching the network — see `docs/protocol-28.md` and the
`report_roundtrip.rs` integration tests.
