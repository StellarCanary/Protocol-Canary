# CF-01: Report contract

Status: frozen for implementation planning, contract version 1. Updated with
the maintainer decisions D-02 (zero-check runs) and the report-provenance
direction for RH-02; see the decision log in [`README.md`](README.md).
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
- In `0.1.1` a run in which every fixture is skipped produces
  `status: "pass"`, `counts.total: 0` and exit code `0`. Reproduced on
  2026-10-09 with `stellar-canary check --protocol 29` against the shipped
  Protocol 28 pack: 7 skipped, 0 executed, pass, exit 0. The maintainer has
  approved changing this in the next release; see section 7.
- Reports from `0.1.1` and earlier carry no `results[].source`. Those releases
  did not use the result cache at all, so every result in them was executed in
  that run; see section 6 for the cache that arrives with `0.2.0`.

## 2. Compatibility classes

| Change | Class | Rule |
|---|---|---|
| Add an optional field, or a new value in an open string field | Additive | Allowed in `schemaVersion` 1. Consumers must ignore fields they do not know. |
| Add a value to a closed enum (`status`, `surface`) | Breaking | Requires `schemaVersion` 2. Existing consumers reject unknown values on purpose. |
| Remove or rename a field, change a type, change the meaning of a value | Breaking | `schemaVersion` 2. |
| Make an optional field required | Breaking | `schemaVersion` 2. |
| Change a default that turns a previously passing invocation into a failing one | Release-boundary change | Does not change the report shape, so it does not bump `schemaVersion`. It needs maintainer approval, a minor-version release, an opt-out flag, and a changelog entry stating the old and new behavior. D-02 is the precedent. Otherwise it must be opt-in. |

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
| `results[].source` | `"live"` or `"cache"` | Whether this result was executed in this run or replayed from the local result cache. Absent means *not recorded* (a report from `0.1.1` or earlier), never live. A value a consumer does not know is read as not recorded. The engine never writes `"unknown"`. Implemented by RH-02 as an optional field under the additive rule. | CF-01 |
| `skipped[].code` | string, lowercase kebab | Stable machine identifier for the skip reason. `reason` stays free text. Initial codes: `protocol-mismatch`, `surface-disabled`, `missing-capability`. | CF-01 |
| `fixturePack` | object | `{ "digest": "sha256:<hex>", "registryVersion": 1, "source": "directory" or "release", "revision": string or null }` | CF-02 |
| `lock` | object | `{ "path": "<relative>", "status": "absent" or "verified", "warnings": [string] }`. `warnings` carries, for example, an engine-version mismatch (D-12), because stderr is not shown to Action users on a passing run. | CF-03 |
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
project root). The wiring was added after `0.1.1` (commit `d12dc89`, 2026-09-23).
`0.1.0` and `0.1.1` created a `CacheStore` but never read or wrote it, so no
released version ever served a stale result.

**Behavior of `main` before the cache fixes (defect, never released).** Verified
on 2026-10-09: the cache key is
`(fixtureId, protocol, project git commit or `commit-dirty`, hash of RPC URL,
observed protocol)`. It does not include the fixture's content. Editing a
fixture file and rerunning returned the previous result: a fixture asserting
`protocolVersion = 30` failed, was edited to `29` (which passes against
Testnet), and the next run still reported `fail`. Entries never expired, RPC
and Soroban results were replayed like any other, and a replayed result looked
identical to a live one.

**Required behavior (RH-01 and RH-02, approved direction).**

1. The key covers everything that determines the result: the fixture file bytes
   and the bytes of any referenced payload, the target protocol, the network
   name, the RPC endpoint, the observed protocol, the tool version, and a
   path-free project fingerprint (type, capabilities, Git commit and dirty
   flag). No absolute path is part of it. The cache has a layout number; an
   entry from another layout, an entry whose stored key differs from the
   requested key, and an unreadable entry are all misses.
2. Offline XDR results may be reused without a time limit, because the key
   contains every input they depend on.
3. RPC and Soroban results are not stored or served by default. A recorded
   answer from a network is not evidence about the network now. A caller may
   opt in with `--live-cache-ttl SECONDS`, and then an entry is served only
   while younger than that; an entry dated in the future is not served.
   `--no-cache` disables reading and writing. There is no default freshness
   window, and it is unrelated to the verification freshness window of CF-04.
4. A replayed result carries `"source": "cache"`; a result executed in this run
   carries `"source": "live"`. The terminal and Markdown reports say how many
   results were replayed.
5. A consumer treats a `"cache"` result as a recording. A report whose passing
   RPC or Soroban results are all `"cache"` is not evidence about the network.

## 7. Failure semantics

Exit codes are unchanged: `0` pass or warning, `1` compatibility failure, `2`
configuration error, `3` execution error, `4` invalid fixture, `5` internal.

| Situation | `status` | Exit | Notes |
|---|---|---|---|
| All executed results pass | `pass` | 0 | |
| Warnings only | `warning` | 0 | `warnings_are_failures` turns this into `fail`. |
| Any `fail` | `fail` | 1 | |
| Any `error` | `error` | 3 | Outranks `fail`. |
| Zero results executed, `0.1.1` and earlier | `pass` | 0 | The old false-green behavior. Not to be relied on. |
| Zero fixtures would execute, releases after `0.1.1`, no `--allow-empty` | none: no report is printed | 2 | Approved as D-02. `check` stops before executing anything. |
| Zero fixtures would execute, releases after `0.1.1`, with `--allow-empty` | `pass` | 0 | Intentional empty run. A warning is printed to stderr. `counts.total` is `0`. |

**D-02 (approved by the maintainer).** From the next release, `check` fails by
default when no fixture would execute. Exit code `2` is the configuration error
code; no result exists to be a compatibility failure (`1`) and nothing failed to
execute (`3`). The error names the cause: the fixtures directory is missing, it
holds no `*.toml` files, or every loaded fixture was skipped because of the
target protocol, a disabled surface, or a capability the project does not
declare. No report is printed, so a consumer cannot mistake the run for a
result, and the JSON report shape is unchanged. The published `0.1.1` behavior
and tag are not rewritten. Because this changes the outcome of an existing
invocation it ships in a minor version and the changelog states the old and new
behavior. A caller that wants an empty run to pass passes `--allow-empty`.
`inspect` and `fixtures` are unaffected: they exist to show why a plan is empty.

Consequence for the Action: with an engine release that contains this change,
the CLI exits `2` with empty stdout. The Action's existing path for "no usable
report" then reports `execution-failed` with the exit code description and the
CLI's stderr, which carries the diagnostic. An Action pinned to `0.1.1` keeps the
old behavior until its `version` default moves.

Rules for consumers (Action, viewer, comparison):

1. Never present a report with `counts.total == 0` as "compatible". Show it as
   "no checks ran", with the skip counts. Such a report now exists only when the
   caller passed `--allow-empty`, or when it was written by `0.1.1` or earlier.
2. Never recompute `status`. Display it. A consumer may verify that `counts`
   agree with `results` and warn if not.
3. An unsupported `schemaVersion`, an unknown `status` or `surface`, a missing
   required field, or a duplicate result identity makes the report invalid. The
   consumer reports "invalid report", not a compatibility result.
4. An invalid report must never produce a green outcome.

Exit code `2` now covers two situations: invalid configuration and an empty
plan. A script that needs to tell them apart reads the message; no separate code
is introduced because the existing error architecture already classes both as
"the run was not set up to produce a result" (maintainer direction).

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

A report that ran nothing (illustrative). The shape is valid, but it can only come from `0.1.1` or earlier, or from a later release run with `--allow-empty`; consumers must never read it as compatibility:

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
| CF-01-D Emit `skipped[].code` | Protocol-Canary | this contract | The scheduler now records a `SkipCause` with the stable codes `protocol-mismatch`, `surface-disabled`, `missing-capability` (RH-03); only the JSON field is missing. |
| CF-01-E Emit `results[].source` | Protocol-Canary | this contract | **Done in RH-02.** |
| CF-01-F Zero-result guard | Protocol-Canary | D-02 | **Done in RH-03** as a default failure with `--allow-empty`; the opt-in design this row used to describe is superseded. |
| CF-01-G Action tolerates reserved fields and rejects duplicate identities | ProtocolCanary-Action | this contract | |
