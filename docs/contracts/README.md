# Shared contracts

These eight documents define the interfaces that the three StellarCanary
repositories share. `Protocol-Canary` hosts all of them, so there is one
definition of each. `ProtocolCanary-Fixtures` and `ProtocolCanary-Action` link
here from their own `docs/contracts.md` and do not restate the rules.

Existing documents stay in force: [`../fixture-contract.md`](../fixture-contract.md)
(fixture format v1), [`../json-report-contract.md`](../json-report-contract.md)
(the exact bytes `0.1.1` emits) and [`../architecture.md`](../architecture.md).
Where a contract here is stricter or adds a rule, it says so.

| ID | Contract | Primary consumers |
|---|---|---|
| [CF-01](cf-01-report.md) | Report compatibility, identity and failure semantics | Engine, Action, viewer |
| [CF-02](cf-02-registry-and-digest.md) | Fixture registry and pack digest | Engine, Fixtures |
| [CF-03](cf-03-lockfile.md) | `.stellar-canary.lock` version 1 | Engine, Action |
| [CF-04](cf-04-verification-evidence.md) | Verification evidence and the Protocol 29 boundary | Fixtures, Engine, viewer |
| [CF-05](cf-05-report-comparison.md) | Report comparison | Engine, Action, viewer |
| [CF-06](cf-06-fixture-releases.md) | Canonical fixture releases | Fixtures, Engine, Action |
| [CF-07](cf-07-project-roots-and-detection.md) | Project roots and capability detection | Engine, Action |
| [CF-08](cf-08-viewer-and-action.md) | Readiness viewer and Action integration | Engine, Action |

## Status and versioning

Each contract is "frozen for implementation planning" at contract version 1.
That means: the rules are stable enough that independent work can start, and a
change to a rule needs a pull request that edits the contract first. It does not
mean the feature exists. Every shape marked "planned", "proposed" or "reserved"
is not implemented in `0.1.1` or on `main`.

Examples in these documents are illustrative. Digests are zeros, versions are
`0.0.0`, hosts are `.invalid`. None of them is a verification result. Blocks
preceded by `<!-- contract-example: ... -->` are checked by tests where a parser
exists (`crates/canary-report/tests/contract_examples.rs` for CF-01).

## Dependency order

CF-01 and CF-07 have no dependencies. CF-02 depends on nothing but is needed by
CF-03, CF-04 and CF-06. CF-05 depends on CF-01. CF-08 depends on CF-01, CF-05 and,
for optional panels, CF-04.

## Pending maintainer decisions

Nothing below is decided. Each is stated in the contract that needs it, with a
default that implementers must not rely on until it is recorded here.

| ID | Question | Where | Proposed default |
|---|---|---|---|
| D-01 | Express additive report fields inside `schemaVersion` 1, or move to 2 | CF-01 | Additive, as written |
| D-02 | Should a run that executes zero checks ever fail by default | CF-01 | No in version 1; opt-in flag; revisit for version 2 |
| D-03 | Registry committed in the Fixtures repository, or generated only at release | CF-02 | Committed, with a CI freshness check |
| D-04 | Exit code for a stale lock | CF-03 | `2` |
| D-05 | Freshness window for live verification | CF-04 | 30 days |
| D-06 | Numeric limits for archives, scans and report sizes | CF-05, CF-06, CF-07 | Values stated in each contract |
| D-07 | Signed releases beyond checksums | CF-06 | Not designed |
| D-08 | Upward project-root discovery when `--project-root` is absent | CF-07 | None; current directory only |
| D-08b | Viewer hosting path | CF-08 | `viewer/` published beside the book |
| D-09 | Behavior of the Protocol 28 RPC identity fixtures now that live networks report 29 | CF-04 | Undecided; options A, B, C in CF-04 |

## Findings that shaped these contracts

Verified on 2026-10-09 against `main` at `1ff7908` (Engine), `828b41c`
(Fixtures) and `0f6caab` (Action). These are defects or gaps, not features, and
are listed so they are fixed as such rather than discovered later.

1. The result cache is wired into `check` (it has been since `v0.1.0`) but its key
   ignores fixture content, so an edited fixture can return a stale result, and a
   cached result looks identical to a live one (CF-01 section 6).
2. `check --protocol 29` with the shipped pack skips everything and reports
   `pass`, exit 0 (CF-01 section 7, CF-04 section 5).
3. The two Protocol 28 RPC fixtures fail against Testnet today because the
   network reports protocol 29 (CF-04 section 5).
4. The fixture loader follows symlinks and does not confine `input_file` and
   `expected_file` to the pack (CF-02 section 1). No runner reads those files yet.
5. The JSON report contract example shows `"skipped": []`, but the CLI omits an
   empty `skipped` (CF-01 section 1).
6. Project detection ignores workspace members, so a virtual Cargo workspace
   detects as `Unknown` (CF-07 section 1).
7. `ROADMAP.md`, the README, `docs/architecture.md` and the mdBook say the cache is
   not wired. Corrected in this branch.
