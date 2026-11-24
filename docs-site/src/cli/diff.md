# `diff`

Compares two previously generated JSON reports and renders the
comparison as machine-readable `diffVersion: 1` JSON (CF-05). Nothing is
re-run and no network call is made — the diff is computed entirely from
the two stored files.

```text
Compare two stored JSON reports and render the diff (CF-05)

Usage: stellar-canary diff <CURRENT_REPORT> <BASELINE_REPORT>

Arguments:
  <CURRENT_REPORT>  Path to the current report (produced by `stellar-canary check --json`)
  <BASELINE_REPORT> Path to the baseline report to compare the current report against

Options:
  -h, --help  Print help
```

Note the argument order: **current first, baseline second**. The diff
answers "what changed since the baseline", so `pass -> fail` (a
regression) is what you get when a test passed in the baseline and fails
now.

## Exit codes (CF-05)

`diff` uses the same [exit-code contract](../exit-codes.md) as `check`
and friends — CF-05 section 5 fixes its numbers to "mirror the existing
meanings of `1` and `2`" — read through the comparison's lens:

| Code | Global contract | `diff` meaning |
|---|---|---|
| `0` | `Pass` | Verdict `no-change` or `improved` — safe to merge. |
| `1` | `CompatibilityFailure` | Verdict `regression` or `coverage-lost`. |
| `2` | `ConfigurationError` | The two reports are **not comparable**, or a report file is missing or invalid (see below). |

## Comparison eligibility

Two reports are only comparable when they carry the **same, supported
`schemaVersion`** and the **same `targetProtocol`**, with no identity
listed twice in either input. Comparing a Protocol 28 report against a
Protocol 29 baseline would otherwise produce a diff full of
cross-protocol noise that looks like a real regression, so `diff`
refuses instead: it prints a diagnostic explaining exactly why (a
`schemaVersion mismatch`, a protocol mismatch, a duplicate identity or
an invalid report) on stderr, produces no JSON, and exits `2`.

## Verdicts

The `verdict` field follows a strict precedence, evaluated in this
order, first match wins:

- **regression** — at least one entry has `regression: true`.
- **coverage-lost** — `missing` is non-empty (a baseline test is absent
  from the current report) and nothing regressed.
- **improved** — at least one entry is `resolved`, `improved` or
  `added`; nothing regressed or vanished.
- **no-change** — nothing differs.

Newly added tests appear in `entries` with an `absent` `from` state;
they never flip the verdict into a regression.

## Output shape

```json
{
  "diffVersion": 1,
  "comparable": true,
  "baseline": { "toolVersion": "0.2.0", "targetProtocol": 28, "status": "pass" },
  "current": { "toolVersion": "0.2.0", "targetProtocol": 28, "status": "fail" },
  "fixturePackChanged": null,
  "observedProtocolChanged": false,
  "emptyBaseline": false,
  "emptyCurrent": false,
  "entries": [
    { "surface": "rpc", "id": "p28-rpc-1", "from": "pass", "to": "fail", "classification": "regressed", "regression": true }
  ],
  "missing": [],
  "summary": { "regressed": 1, "resolved": 0, "added": 0, "removed": 0, "unchanged": 0, "missing": 0 },
  "verdict": "regression"
}
```

| Field | Meaning |
|---|---|
| `diffVersion` | Integer, currently `1`. |
| `comparable` | Always `true` when a document is produced: an ineligible pair exits `2` with no output at all. |
| `baseline` / `current` | Each side's `toolVersion`, `targetProtocol` and overall `status`. |
| `fixturePackChanged` | `null` — unknown until reports carry a `fixturePack` digest (never silently `false`). |
| `observedProtocolChanged` | `true` only when both reports observed a protocol and they differ. |
| `emptyBaseline` / `emptyCurrent` | Whether that side had no results (an empty side cannot establish a regression). |
| `entries` | Every identity from either report: `surface`, `id`, `from`/`to` state (`pass`, `warning`, `fail`, `error`, `skipped`, `absent`), the CF-05 `classification` and the `regression` flag. Sorted by `(surface, id)` in UTF-8 byte order. |
| `missing` | Baseline identities that passed but are absent or skipped now (coverage lost). |
| `summary` | Counts over `entries` and `missing`. |
| `verdict` | One of `no-change`, `improved`, `coverage-lost`, `regression`. |

Serialization is deterministic: the same pair of reports always
produces byte-identical JSON, so the output can be committed, diffed, or
hash-checked in CI.

## Example

```bash
stellar-canary check --json > current.json        # on the PR branch
git stash && stellar-canary check --json > baseline.json && git stash pop

stellar-canary diff current.json baseline.json
echo $?   # 0 no-change/improved, 1 regression/coverage-lost, 2 invalid input
```
