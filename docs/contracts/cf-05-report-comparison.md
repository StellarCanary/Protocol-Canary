# CF-05: Report comparison

Status: frozen for implementation planning, contract version 1.
Authoritative for: result identity across reports, status transitions,
comparison output and baseline compatibility.

## 1. Purpose and existing behavior

No comparison exists. `stellar-canary` has `check`, `inspect`, `fixtures`,
`report` and `version` (`crates/canary-cli/src/cli.rs`). `report` only re-renders
one stored report. This contract defines how two CF-01 reports are compared so a
CI job can fail on a new regression without failing on a known one, and so the
CLI, Action and viewer agree.

The comparison runs in the CLI. The Action and the viewer display its output;
neither implements transition logic (CF-08).

## 2. Inputs and eligibility

Inputs are a **baseline** and a **current** report, both valid under CF-01.

| Condition | Outcome |
|---|---|
| Either report invalid (CF-01 section 7 rule 3) | Comparison refused, exit `2`, message names which input. |
| `schemaVersion` differs between the two | Refused, exit `2`. |
| `targetProtocol` differs | Refused by default, exit `2`: results about different protocols are not comparable. A flag may allow it later; if it does, the output sets `comparable: false` for the affected results and never reports them as resolved. |
| `fixturePack.digest` present in both and different | Comparable. The output sets `fixturePackChanged: true` and lists added and removed ids as `added` and `removed`, which is what they are. |
| `fixturePack` absent in either | Comparable. `fixturePackChanged` is `null` (unknown, not false). |
| `network.observedProtocol` differs | Comparable. `observedProtocolChanged: true`; informational. |
| `project.type` differs | Comparable. Informational. |
| Either report has `counts.total == 0` | Comparable, but `emptyBaseline` or `emptyCurrent` is set to `true` and no result is called resolved or regressed on the basis of an empty side. |

`toolVersion` differences are recorded, never gating.

## 3. Result identity

The identity of a result is `(surface, id)` where `id` is `fixtureId` or `testId`
when `fixtureId` is absent. Skipped entries use `(surface, fixtureId)` with
state `skipped`. A fixture that appears in `results` in one report and in
`skipped` in the other is one identity with two states.

## 4. States and transitions

State of an identity in a report: `pass`, `warning`, `fail`, `error`, `skipped`,
or `absent` (not in `results` and not in `skipped`).

Severity order: `pass` < `warning` < `fail` < `error`. `skipped` and `absent`
are not on that scale.

| Baseline to current | Classification | Counts as regression |
|---|---|---|
| X to X | `unchanged` | no |
| `pass` to `warning` | `degraded` | no, unless policy `warnings_are_failures` was in effect in the current run |
| `pass` or `warning` to `fail` or `error` | `regressed` | yes |
| `fail` to `error` | `degraded` | yes (still failing, worse) |
| `error` to `fail` | `improved` | no |
| `fail` or `error` to `pass` or `warning` | `resolved` | no |
| `warning` to `pass` | `improved` | no |
| `absent` to `pass` | `added` | no |
| `absent` to `warning`, `fail` or `error` | `added-failing` | yes |
| `pass` or `warning` to `absent` | `removed` | no, but see `missing` below |
| `fail` or `error` to `absent` | `removed-failing` | no, never reported as resolved |
| any of `pass`, `warning` to `skipped` | `skipped` | no, but flagged `coverage-lost` |
| `fail` or `error` to `skipped` | `skipped-failing` | yes: a failing check that stopped running is not resolved |
| `skipped` to `pass` or `warning` | `added` | no |
| `skipped` to `fail` or `error` | `added-failing` | yes |
| `skipped` to `absent`, `absent` to `skipped` | `unchanged-skip` | no |

**Missing checks.** An identity that passed in the baseline and is `removed` or
`skipped` in current is listed under `missing`. A comparison with a non-empty
`missing` set never prints a success-only summary. Removing a failing fixture is
not a fix, hence `removed-failing`.

**Skip reasons.** When `skipped[].code` exists (CF-01), a change of code between
runs is reported under `skipReasonChanged`. Before that field exists, reasons are
free text and are compared only for display.

**Cached results.** If `results[].source` exists and is `cache` on either side,
the entry carries `sourceChanged` and the summary notes it. A transition based on
a cached result is still reported; it is not suppressed.

## 5. Output (`diffVersion` 1)

Deterministic: entries sorted by `(surface, id)`, UTF-8 byte order; no
timestamps, no paths.

<!-- contract-example: diff-v1 -->
```json
{
  "diffVersion": 1,
  "comparable": true,
  "baseline": { "toolVersion": "0.1.1", "targetProtocol": 28, "status": "pass" },
  "current": { "toolVersion": "0.1.1", "targetProtocol": 28, "status": "fail" },
  "fixturePackChanged": null,
  "observedProtocolChanged": false,
  "emptyBaseline": false,
  "emptyCurrent": false,
  "entries": [
    { "surface": "rpc", "id": "example-rpc-fixture", "from": "pass", "to": "fail", "classification": "regressed", "regression": true }
  ],
  "missing": [],
  "summary": { "regressed": 1, "resolved": 0, "added": 0, "removed": 0, "unchanged": 0, "missing": 0 },
  "verdict": "regression"
}
```

`verdict` is evaluated in this order, first match wins: `regression` when any
entry has `regression: true`; `coverage-lost` when `missing` is non-empty;
`improved` when any entry is `resolved`, `improved` or `added`; otherwise
`no-change`. `incomparable` appears only if a future flag allows a comparison
across target protocols (section 2); today that case exits `2` with no output.

Exit codes for a future `diff` command: `0` verdict is not `regression`
and no coverage was lost; `1` regression, or `coverage-lost` when the caller asked for strict mode; `2` incomparable or invalid input. These mirror the existing
meanings of `1` and `2`.

## 6. Security and trust boundaries

Both inputs are untrusted files. Size cap on each (proposed 10 MiB, to be confirmed by the maintainers), no network access, no path in output. Strings copied into
output (`id`) are from reports and are escaped by whatever renders them.

## 7. Validation and test requirements

- A golden table test covering every row of section 4 once, plus
  `absent`/`absent`.
- Incompatible baseline tests for each row of section 2.
- A determinism test: shuffling `results` in either input gives identical bytes.
- Duplicate identities in an input are rejected before comparison.
- The corpus must include a real report from `v0.1.1` as baseline (CF-01-C).

## 8. Cross-repository dependencies

CF-01 (identity and invalid-report rules), CF-02 (`fixturePack`). The Action
(AC baseline input) shells out to the CLI. The viewer (CF-08) can import two
reports and must show the CLI's classification table, which it reimplements only
as display; hidden divergence is prevented by a shared golden test file.

## 9. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CF-05-A Domain model and classifier (library only) | Protocol-Canary | this contract |
| CF-05-B Eligibility checks (section 2) | Protocol-Canary | CF-05-A |
| CF-05-C JSON renderer for `diffVersion` 1 | Protocol-Canary | CF-05-A |
| CF-05-D Terminal and Markdown renderers | Protocol-Canary | CF-05-A |
| CF-05-E `diff` subcommand wiring and exit codes | Protocol-Canary | CF-05-B, CF-05-C |
| CF-05-F Shared golden corpus of report pairs and expected outputs | Protocol-Canary | this contract |
| CF-05-G Action: baseline input and summary section | ProtocolCanary-Action | CF-05-E released |
