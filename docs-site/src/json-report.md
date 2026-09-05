# JSON Report

This documents `stellar-canary check --json` (equivalently `--format json`)
output, as implemented in `crates/canary-report/src/json.rs` and verified
by running the real `v0.1.1` binary against the real
`ProtocolCanary-Fixtures` `protocol-28` pack. This is the exact contract
[`ProtocolCanary-Action`](./github-action.md) consumes to build its GitHub
job summary and annotations.

`schemaVersion` is currently **`1`**. A purely additive field (like `git`
and `counts` were, historically) does not bump it; an incompatible shape
change would.

## Real example (PASS)

```json
{
  "schemaVersion": 1,
  "toolVersion": "0.1.1",
  "targetProtocol": 28,
  "project": { "name": "Protocol-Canary", "type": "unknown" },
  "network": { "name": "testnet", "observedProtocol": 28 },
  "status": "pass",
  "counts": { "total": 5, "passed": 5, "failed": 0, "warnings": 0, "errors": 0, "skipped": 0 },
  "results": [
    { "testId": "p28-xdr-cap83-empty-tx-set", "protocol": 28, "surface": "xdr", "status": "pass", "summary": "StellarValue round-tripped byte-for-byte", "durationMs": 0, "fixtureId": "p28-xdr-cap83-empty-tx-set" }
  ],
  "git": { "commit": "ffa072cb14c682bfa5ebf158ab9bbe6058962aef", "branch": "main", "isDirty": false }
}
```

(Full five-result example in [Your First Check](./first-check.md#5-run-json-mode).)

## Field reference

| Field | Type | Always present? | Notes |
|---|---|---|---|
| `schemaVersion` | integer | yes | Currently `1`. |
| `toolVersion` | string | yes | The CLI's own semver, independent of `schemaVersion`/`targetProtocol`. |
| `targetProtocol` | integer | yes | The protocol this run checked against. |
| `project.name` | string | yes | Directory name of the project root. |
| `project.type` | string | yes | One of `soroban`, `rpc-consumer`, `stellar-sdk`, `generic-stellar`, `unknown`. |
| `network` | object \| absent | only when `rpc` or `soroban` checks are enabled | Omitted entirely for a fully offline (XDR-only) run — its absence *is* the offline signal. |
| `network.name` | string | when `network` present | `testnet`, `mainnet`, `futurenet`, or a custom network name. |
| `network.observedProtocol` | integer \| absent | when `network` present and `getNetwork` succeeded | Absent if the live call failed. Compare against `targetProtocol` to detect a protocol mismatch — do not assume they match. |
| `status` | string | yes | `pass`, `warning`, `fail`, or `error`. `error` means at least one result is `error` (an execution problem) and **overrides** what the pass/fail counts alone would suggest — the same precedence the process exit code follows. |
| `counts` | object | yes | Aggregate over `results` only; `skipped` fixtures are counted separately and never included in `counts.total`. Always re-derivable from `results[].status`. |
| `counts.total` | integer | yes | `results.length`. |
| `counts.passed` / `.failed` / `.warnings` / `.errors` | integer | yes | Count of each status among `results`. |
| `counts.skipped` | integer | yes | `skipped.length`. |
| `results` | array | yes | One entry per fixture that actually ran. Empty is valid (no fixtures found) and is a pass. |
| `results[].testId` / `.fixtureId` | string | yes (`fixtureId` may be `null`) | Currently always equal to each other and to the fixture's `id`. |
| `results[].protocol` | integer | yes | The fixture's own declared protocol. |
| `results[].surface` | string | yes | `xdr`, `rpc`, or `soroban` (lowercase). |
| `results[].status` | string | yes | `pass`, `warning`, `fail`, `error`, or `skipped` (in practice, `results[]` entries are never `skipped` — see `skipped` below). |
| `results[].summary` | string | yes | One-line human-readable outcome. |
| `results[].details` | string \| absent | only when there's something to say | Present for failures/errors; absent (not `null`) when there's nothing further. |
| `results[].durationMs` | integer | yes | Wall-clock time for that fixture. |
| `skipped` | array | only when non-empty | Fixtures the planner decided not to run, and why. Omitted entirely (not `[]`) when there are none. |
| `skipped[].fixtureId` / `.surface` / `.reason` | string | yes | `reason` is human-readable, e.g. `"fixture targets protocol 27, this run targets protocol 28"`. |
| `git` | object | yes | Always present, with `null` fields when unavailable (not a Git repo, detached HEAD) — never omitted, never an error. |
| `git.commit` / `.branch` | string \| null | yes | |
| `git.isDirty` | boolean \| null | yes | |

## Backward compatibility around `counts`

`counts` and `git` were added after `schemaVersion` 1 was first shipped,
as purely additive fields. A consumer reading an older report that
predates `counts` should derive it from `results[].status` rather than
assume the field exists — this is exactly what
[`ProtocolCanary-Action`](./github-action.md) does (see its
`tests/unit/output.test.ts`), so it works against both the `0.1.0` and
`0.1.1` `Protocol-Canary` releases.

## Real example (execution error)

```text
Status: error
```

```json
"results": [
  { "testId": "p28-rpc-network", "surface": "rpc", "status": "error",
    "summary": "failed to call GetNetwork",
    "details": "network transport error calling getNetwork: error sending request for url (...)" }
]
```

`network.observedProtocol` is correctly **absent** here — the live call
never returned a value — while `network.name` is still present.

## What a downstream consumer should key off of

- **Overall pass/fail**: `status`, not the process exit code, if you're
  parsing a saved file rather than checking `$?` directly — remember
  `error` takes precedence.
- **Per-check detail** (a PR comment, an annotation): iterate `results`,
  using `surface` to group and `status`/`summary`/`details` for content.
- **"N/M passed" summaries**: `counts.passed` / `counts.total` directly —
  never compute a percentage without stating the denominator.
- **Protocol mismatch detection**: compare `targetProtocol` to
  `network.observedProtocol` when both are present.

## Reading a saved report back

```bash
stellar-canary report result.json --format markdown
```

parses exactly this shape and re-renders it without touching the network
— see [`report`](./cli/report.md).
