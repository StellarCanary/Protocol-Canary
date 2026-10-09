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

## Decision log

### Approved by the maintainer

| ID | Decision | Recorded in | Consequences |
|---|---|---|---|
| D-02 | In the next release, `check` fails by default when it would execute zero fixtures. Exit code `2`. `--allow-empty` opts out. The published `0.1.1` behavior and tag stay as they are. | [CF-01 section 7](cf-01-report.md) | Behavior change at a release boundary, shipped in a minor version with a changelog entry. The JSON report shape does not change. No report is printed on refusal. The error names the cause. The Action receives `execution-failed` with the CLI diagnostic once its engine version moves past `0.1.1`. Scripts that relied on an empty run passing must add `--allow-empty`. |
| D-06 | Numeric resource limits stated in the contracts are approved: archive extraction (maximum 2,000 entries, 16 MiB uncompressed total, 1 MiB per file, depth 8), project scanning (depth 4, 2,000 directories visited, 10,000 directory entries read, 1 MiB per manifest, maximum 64 workspace members), RPC response cap (16 MiB; request parameter bounds: at most 50 keys, limit at most 50, at most 2 filters), report comparison input cap (10 MiB per report), and fingerprint manifest size limit (1 MiB), retaining any other numeric bounds explicitly stated in CF-05, CF-06, CF-07, CA-01 and CA-04. Limit violations must fail safely or stop the affected operation as already specified by each contract; they must not silently truncate security-sensitive input. Values must not be changed merely to simplify implementation; future changes require a new maintainer decision. | [CF-05 section 6](cf-05-report-comparison.md), [CF-06 section 6](cf-06-fixture-releases.md), [CF-07 section 5](cf-07-project-roots-and-detection.md), [CA-01 sections 3 and 6](ca-01-rpc-requests-and-assertions.md), [CA-04 section 3](ca-04-project-fingerprint.md) | Enforces bounded resource limits across extraction, scanning, comparison and RPC handling. Any limit violation fails safely or halts the operation with a diagnostic rather than silently truncating security-sensitive input. Values cannot be changed without a new maintainer decision. |
| D-09 | Option C: preserve the verified Protocol 28 pack; build Protocol 29 coverage separately from real specifications and real captures. | [CF-04 section 5](cf-04-verification-evidence.md) | No Protocol 28 assertion is changed to expect 29. Historical evidence stays. An endpoint reporting 29 cannot serve as a Protocol 28 live environment, and this is documented. Mismatch diagnostics stay visible. No Protocol 29 fixture without an authoritative source, a real capture and a verification record. No invented CAPs. |
| D-10 | Diagnostics artifacts in ProtocolCanary-Action are opt-in and disabled by default. Preserve the existing CF-08 security model: no environment dump, best-effort secret redaction, bounded diagnostic output (last 64 KiB of stdout and stderr), distinct diagnostics artifact (`stellar-protocol-canary-diagnostics`), and no diagnostics artifact unless the user explicitly enables it. | [CF-08 section 3.5](cf-08-viewer-and-action.md) | Diagnostic logs will not be uploaded on public repositories by default, avoiding potential leakage of execution details or redacted credentials. Implementations and draft issue A-003 follow this opt-in policy. |
| (RH-02) | Replayed results are marked in the report, and live results are not replayed by default. Applied as an optional additive field. | [CF-01 sections 3 and 6](cf-01-report.md) | `results[].source`. This applies the additive rule of D-01 to one field. It does not settle D-01 for later fields. |

### Still pending

Nothing below is decided. Each is stated in the contract that needs it, with a
default that implementers must not rely on until it is recorded here.

| ID | Question | Where | Proposed default |
|---|---|---|---|
| D-01 | Express additive report fields inside `schemaVersion` 1, or move to 2, as a general policy | CF-01 | Additive, as written |
| D-03 | Registry committed in the Fixtures repository, or generated only at release | CF-02 | Committed, with a CI freshness check |
| D-04 | Exit code for a stale lock | CF-03 | `2` (consistent with D-02) |
| D-05 | Freshness window for live verification evidence (not the result cache) | CF-04 | 30 days |
| D-07 | Signed releases beyond checksums | CF-06 | Not designed |
| D-08 | Upward project-root discovery when `--project-root` is absent | CF-07 | None; current directory only |
| D-08b | Viewer hosting path | CF-08 | `viewer/` published beside the book |
| D-11 | Which hosts may a fixture-pack download redirect to? | CF-06 | Unresolved (see review item S2) |
| D-12 | Is a lockfile `tool.version` mismatch a warning or a failure? | CF-03 | Warning in version 1 (see review item S3) |

The proposed directions, their security sensitivity and the evidence still needed
are collected in [`decisions.md`](decisions.md).

## Review of the remaining proposed defaults (2026-10-09)

This pass looked for contradictions between the contracts, between the contracts
and the approved decisions, and for security questions the earlier text left
open. Nothing here is approved by this review; open items are in the table above.

| # | Finding | Resolution |
|---|---|---|
| I1 | CF-01 said a default change that turns a passing run into a failing one must be opt-in. D-02 is exactly such a change. | Rule rewritten in CF-01 section 2 as a release-boundary change with conditions. |
| I2 | CF-01 section 7 treated the zero-result false green as permanent and planned an opt-in flag. | Replaced by D-02 in CF-01 section 7. Unit CF-01-F is marked done and superseded. |
| I3 | Exit code `2` now means both invalid configuration and an empty plan, and CF-03 also proposes `2` for a stale lock. A caller cannot tell them apart by code. | Accepted by the maintainer for the empty plan; messages are the discriminator. D-04 keeps the same code for consistency; if a script-friendly distinction is wanted, that is a new decision, not an edit. |
| I4 | Two different digests exist: the engine's result-cache fixture digest (internal, per fixture, not exposed) and the CF-02 pack digest. | CF-02 section 5 now says they are unrelated and that the cache digest is never written to a lockfile, registry or report. |
| I5 | CF-02 defined pack membership as every `*.toml`, including inside `.git`; the loader now skips `.git`. | CF-02 section 2 updated in the loader change. |
| I6 | CF-01 reserved `results[].source` with an `unknown` reading but no rule for unknown values. | Rule added: an unknown value is read as not recorded; the engine never writes `unknown`. |
| I7 | CF-03 rejects unknown lock keys while CF-01 consumers ignore unknown report fields. | Intentional and now stated: a lock is a pin whose unknown key could drop a pin; a report is a record. |
| I8 | CF-04 freshness (30 days proposed) and the live cache TTL (user-set, no default) could be confused. | Stated as unrelated in CF-01 section 6. |
| S1 | CF-08 let the Action upload a diagnostics artifact containing stderr tails. Artifacts on a public repository can be downloaded by anyone who can read the repository, and pattern-based redaction is best effort (a cargo error can echo a URL with credentials). | Opt-in input, default off, and tail limited to what the Action itself generated. Approved as D-10. |
| S2 | CF-06 allowed GitHub release hosts without naming them. A release download redirects to a different host, and an over-narrow list breaks it while an over-wide one weakens the check. | Pending D-11. The list must be taken from observed redirects of a real release, and tested. |
| S3 | CF-03 treats a `tool.version` mismatch as a warning, which means a lock does not prevent running a different engine. | Pending D-12. Version 1 default stays a warning because the lock pins fixtures, not the tool. |
| S4 | CF-08 delivers its Content-Security-Policy as a `<meta>` tag because GitHub Pages cannot set headers. A meta CSP does not support `frame-ancestors` or reporting. | Stated in CF-08 section 2.2; `connect-src 'none'` and `script-src 'self'` do work from a meta tag, which is what the no-upload guarantee relies on. |
| S5 | CF-05 and CF-08 cap report size at 10 MiB, but JSON nesting depth is separate. | `serde_json` limits recursion to 128 levels by default and `JSON.parse` is iterative enough for 10 MiB; the viewer test corpus must include a deeply nested report. Limits approved under D-06. |
| S6 | `getLatestLedger` responses carry `metadataXdr` of about 1.28 million characters on Mainnet. The engine downloads and discards it, so an RPC-heavy run on a metered connection pays for it. | Not a contract change. Recorded in CA-01 as a cost to weigh before adding more fixtures on that method. |

## Contract addenda (CA)

Addenda complete contracts that were missing a specification the backlog needs.
They are frozen the same way as the eight contracts.

| ID | Addendum |
|---|---|
| [CA-01](ca-01-rpc-requests-and-assertions.md) | RPC request parameters and response assertions (`getVersion` in older notes means `getVersionInfo`) |
| [CA-02](ca-02-fixtures-json.md) | `stellar-canary fixtures --format json` |
| [CA-03](ca-03-inspect-json.md) | `stellar-canary inspect --format json` |
| [CA-04](ca-04-project-fingerprint.md) | Project fingerprint |

## Findings that shaped these contracts

Verified on 2026-10-09 against `main` at `1ff7908` (Engine), `828b41c`
(Fixtures) and `0f6caab` (Action). These are defects or gaps, not features, and
are listed so they are fixed as such rather than discovered later. The last
column is the Phase 2 work item that addresses each one; the pull requests are
listed in the Phase 2 report, not here, so this file does not go stale.

| # | Finding | Addressed by |
|---|---|---|
| 1 | The result cache was wired into `check` on `main` after `v0.1.1` (commit `d12dc89`; `0.1.0` and `0.1.1` never used it), and that unreleased wiring keyed on fixture id but not fixture content, so an edited fixture returned a stale result, and a cached result looked identical to a live one (CF-01 section 6). Nothing released was affected. | RH-01 (key), RH-02 (provenance and live policy) |
| 2 | `check --protocol 29` with the shipped pack skipped everything and reported `pass`, exit 0 (CF-01 section 7, CF-04 section 5). | RH-03 (D-02) |
| 3 | The two Protocol 28 RPC fixtures fail against Testnet because the network reports protocol 29 (CF-04 section 5). | Not a defect. Decided as D-09 Option C and documented. |
| 4 | The fixture loader followed symlinks and did not confine `input_file` and `expected_file` to the pack (CF-02 section 1). | RH-04 |
| 5 | The JSON report contract example showed `"skipped": []`, but the CLI omits an empty `skipped` (CF-01 section 1). | Corrected in the first contract commit. |
| 6 | Project detection ignores workspace members, so a virtual Cargo workspace detects as `Unknown` (CF-07 section 1). | Open. Contributor work (CF-07-C). |
| 7 | `ROADMAP.md`, the README, `docs/architecture.md` and the mdBook said the cache is not wired. That was true of `0.1.x` and false of `main` after `d12dc89`. | Rewritten for the `0.2.0` behavior, with the release it starts in stated. |
| 8 | Target, observed and fixture protocols had no regression test keeping them apart. | RH-05 |
