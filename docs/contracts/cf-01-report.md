# CF-01: Report contract

Status: frozen for implementation planning, contract version 1.
Authoritative for: report compatibility, identity and failure semantics.
Field-by-field reference for the current output stays in
[`../json-report-contract.md`](../json-report-contract.md); where the two
differ, this document describes the rule and that document describes the
bytes `0.1.1` actually emits.

## 1. Purpose and existing behavior

`stellar-canary check --json` prints one JSON document
(`crates/canary-report/src/json.rs`, `SCHEMA_VERSION = 1`). The Action
(`ProtocolCanary-Action/src/output.ts`) and `stellar-canary report` consume it.
Observed facts that this contract builds on, each checked against the source or
a real run on 2026-10-09:

- `JsonReporter::parse` rejects any `schemaVersion` other than `1`
  (`UnsupportedSchemaVersion`), rejects unknown `surface` and `status`
  spellings, and ignores unknown extra fields (no `deny_unknown_fields`).
- `counts` and `git` default when absent, so reports written before `counts`
  existed still parse. `verbose` defaults to `false`.
- `skipped` is omitted entirely when empty (`skip_serializing_if`), as the field
  table in `json-report-contract.md` says. The first example in that document
  showed `"skipped": []`, which the CLI does not print; that example is corrected
  in the same branch. Consumers must treat an absent `skipped` as an empty list.
- `status` is the overall outcome. `error` outranks the policy decision, the
  same precedence as exit code `3` (`policy.rs: exit_code_for_run`).
- `testId` and `fixtureId` are equal today; `fixtureId` may be absent.
- A run in which every fixture is skipped produces `status: "pass"`,
  `counts.total: 0` and exit code `0`. Reproduced on 2026-10-09 with
  `stellar-canary check --protocol 29` against the shipped Protocol 28 pack:
  7 skipped, 0 executed, pass, exit 0. See section 7.
- Nothing in the report says whether a result was executed in this run or
  replayed from the local result cache. See section 6.

## 2. Compatibility classes

| Change | Class | Rule |
|---|---|---|
| Add an optional field, or a new value in an open string field | Additive | Allowed in `schemaVersion` 1. Consumers must ignore fields they do not know. |
| Add a value to a closed enum (`status`, `surface`) | Breaking | Requires `schemaVersion` 2. Existing consumers reject unknown values on purpose. |
| Remove or rename a field, change a type, change the meaning of a value | Breaking | `schemaVersion` 2. |
| Make an optional field required | Breaking | `schemaVersion` 2. |
| Change a default that turns a previously passing run into a failing one | Breaking | Must be opt-in in `schemaVersion` 1 (see D-02). |

`toolVersion` is informational and never used for feature detection by
consumers. A consumer that needs a feature checks for the field.

## 3. Required and optional fields (version 1)

Required in every report written by `0.1.1` and later: `schemaVersion`,
`toolVersion`, `targetProtocol`, `project.name`, `project.type`, `status`,
`counts` (all six members), `results` (may be empty), and per result `testId`,
`protocol`, `surface`, `status`, `summary`, `durationMs`.

Optional: `network` (absent for a fully offline run), `network.observedProtocol`,
`network.error`, `results[].details`, `results[].fixtureId`, `skipped`, `git`,
`verbose`.

A consumer may accept a report that lacks `counts` or `git` (older output) and
must derive `counts` from `results`. It must not invent `network` or
`observedProtocol`.

### Reserved additive fields

These names are reserved so independent work does not collide. None is emitted
by `0.1.1`. Each is optional in version 1. Shapes are normative once an
implementing change lands; until then they are a plan.

| Field | Type | Meaning | Owner contract |
|---|---|---|---|
| `results[].source` | `"live"` or `"cache"` | Whether this result was executed in this run or replayed from the local cache. Absent means unknown, not live. | CF-01 |
| `skipped[].code` | string, lowercase kebab | Stable machine identifier for the skip reason. `reason` stays free text. Initial codes: `protocol-mismatch`, `surface-disabled`, `missing-capability`. | CF-01 |
| `fixturePack` | object | `{ "digest": "sha256:<hex>", "registryVersion": 1, "source": "directory" or "release", "revision": string or null }` | CF-02 |
| `lock` | object | `{ "path": "<relative>", "status": "absent" or "verified" }` | CF-03 |
| `results[].verification` | object | Copy of the fixture's verification record. Never synthesized. | CF-04 |
| `run` | object | `{ "toolBuildCommit": string or null }` | CF-01 |

## 4. Identity

- A result is identified inside one report by `(surface, fixtureId)`, using
  `testId` when `fixtureId` is absent. The loader rejects duplicate fixture ids
  across the whole tree, so this is unique per run. A report in which two
  results share an identity is invalid and must be rejected by consumers.
- Result identity across reports (comparison) is defined in CF-05.
- Report identity (what run produced this) is the tuple
  `(toolVersion, targetProtocol, fixturePack.digest when present, git.commit,
  git.isDirty)`. Absolute paths, hostnames and timestamps are not part of
  identity. The report currently contains no timestamp; adding one is not part
  of version 1 because it would break byte-for-byte comparison of otherwise
  identical runs.
- `durationMs` is not stable and must be ignored by any equality comparison.

## 5. Ordering

`results` appear in plan order: XDR, then RPC, then Soroban, each in fixture
load order, which is sorted by path (`load_directory` sorts paths). Consumers
must not rely on order for correctness. Producers must keep it deterministic.

## 6. Cached results

`CacheStore` is wired into `check` (`canary-runner/src/execution.rs`,
`canary-cli/src/commands.rs`, cache directory `.stellar-canary-cache` under the
project root). The `ROADMAP.md`, `docs/architecture.md` and `CHANGELOG.md`
"known gap" text saying otherwise is out of date (corrected in the same branch).

Verified on 2026-10-09: the cache key is `(fixtureId, protocol, project git
commit or `commit-dirty`, hash of RPC URL, observed protocol)`. It does not
include the fixture's content. Editing a fixture file in a dirty working tree
and rerunning returns the previous result: a fixture asserting
`protocolVersion = 30` failed, was edited to `29` (which passes against Testnet),
and the next run still reported `fail`. There is also no expiry. Consequences
for this contract:

- A consumer cannot currently tell a replayed result from a live one.
- A passing replayed live-network result is not evidence about the network now.
- `results[].source` (section 3) is the additive fix for the first point. The
  cache key correction is an engine defect tracked separately from this
  contract; the contract only requires that once `source` exists, a replayed
  result carries `"cache"`.

## 7. Failure semantics

Exit codes are unchanged: `0` pass or warning, `1` compatibility failure, `2`
configuration error, `3` execution error, `4` invalid fixture, `5` internal.

| Situation | `status` | Exit | Notes |
|---|---|---|---|
| All executed results pass | `pass` | 0 | |
| Warnings only | `warning` | 0 | `warnings_are_failures` turns this into `fail`. |
| Any `fail` | `fail` | 1 | |
| Any `error` | `error` | 3 | Outranks `fail`. |
| Zero results executed (empty fixtures dir, or every fixture skipped) | `pass` | 0 | **Known false-green hazard.** Today indistinguishable from a real pass except by `counts.total == 0`. |

Rules for consumers (Action, viewer, comparison):

1. Never present a report with `counts.total == 0` as "compatible". Show it as
   "no checks ran", with the skip counts.
2. Never recompute `status`. Display it. A consumer may verify that `counts`
   agree with `results` and warn if not.
3. An unsupported `schemaVersion`, an unknown `status` or `surface`, a missing
   required field, or a duplicate result identity makes the report invalid. The
   consumer reports "invalid report", not a compatibility result.
4. An invalid report must never produce a green outcome.

Opt-in guard (planned, version 1 compatible): a `check` flag that makes a run
with zero executed results exit `2`. The default stays unchanged in version 1.
See D-02 for the decision on changing the default.

## 8. Security and trust boundaries

Reports are untrusted input to the Action and the viewer. `summary`, `details`,
`reason`, `project.name` and `network.name` are arbitrary text: render as text,
never as HTML or Markdown without escaping, never as a GitHub workflow command
without neutralizing `::`. Reports contain no secrets by design. `git.branch`
and `project.name` can reveal private repository names; that matters for CF-08
sharing guidance.

## 9. Examples (illustrative, not verification results)

<!-- contract-example: report-v1-valid -->
```json
{
  "schemaVersion": 1,
  "toolVersion": "0.1.1",
  "targetProtocol": 28,
  "project": { "name": "example-project", "type": "soroban" },
  "status": "pass",
  "counts": { "total": 1, "passed": 1, "failed": 0, "warnings": 0, "errors": 0, "skipped": 0 },
  "results": [
    {
      "testId": "example-xdr-fixture",
      "protocol": 28,
      "surface": "xdr",
      "status": "pass",
      "summary": "illustrative result",
      "durationMs": 1,
      "fixtureId": "example-xdr-fixture"
    }
  ],
  "git": { "commit": null, "branch": null, "isDirty": false }
}
```

A report that ran nothing (illustrative; this is the false-green shape):

<!-- contract-example: report-v1-valid -->
```json
{
  "schemaVersion": 1,
  "toolVersion": "0.1.1",
  "targetProtocol": 29,
  "project": { "name": "example-project", "type": "unknown" },
  "status": "pass",
  "counts": { "total": 0, "passed": 0, "failed": 0, "warnings": 0, "errors": 0, "skipped": 1 },
  "results": [],
  "skipped": [
    { "fixtureId": "example-xdr-fixture", "surface": "xdr", "reason": "fixture targets protocol 28, this run targets protocol 29" }
  ],
  "git": { "commit": null, "branch": null, "isDirty": false }
}
```

Invalid: unsupported version.

<!-- contract-example: report-v1-invalid -->
```json
{
  "schemaVersion": 2,
  "toolVersion": "9.9.9",
  "targetProtocol": 28,
  "project": { "name": "example-project", "type": "soroban" },
  "status": "pass",
  "results": []
}
```

## 10. Validation and test requirements

- The `report-v1-valid` blocks in this file must parse with
  `JsonReporter::parse`, and the `report-v1-invalid` block must be rejected
  (test: `crates/canary-report/tests/contract_examples.rs`).
- A corpus of real reports from `v0.1.0` and `v0.1.1` is required before any
  change to the parser (see unit CF-01-C).
- Any additive field must have a test that an older parser, simulated by
  `JsonReporter::parse` at `main` before the change, still accepts the output.
- Property: for any report, `counts` equals the tally of `results` statuses.

## 11. Cross-repository dependencies

- `ProtocolCanary-Action/src/output.ts` mirrors sections 3 and 7 in
  `parseReport`. Its `SUPPORTED_SCHEMA_VERSION` must equal `SCHEMA_VERSION`.
- CF-05 consumes section 4. CF-08 consumes sections 7 and 8.
- `ProtocolCanary-Fixtures` is not a consumer of reports.

## 12. Independent implementation units

| Unit | Repo | Depends on | Notes |
|---|---|---|---|
| CF-01-A Publish a JSON Schema for report v1 matching section 3 | Protocol-Canary | this contract | Schema must accept `0.1.1` output and ignore unknown properties. |
| CF-01-B Conformance test: reporter output validates against the schema | Protocol-Canary | CF-01-A | |
| CF-01-C Corpus of real `v0.1.0` and `v0.1.1` reports and parse tests | Protocol-Canary | none | Needs the tagged binaries; do not hand-write them. |
| CF-01-D Emit `skipped[].code` | Protocol-Canary | this contract | Scheduler already distinguishes the three causes. |
| CF-01-E Emit `results[].source` | Protocol-Canary | this contract | Independent of the cache key fix. |
| CF-01-F Opt-in zero-result guard | Protocol-Canary | D-02 | |
| CF-01-G Action tolerates reserved fields and rejects duplicate identities | ProtocolCanary-Action | this contract | |
