# CF-04: Verification evidence and the Protocol 29 boundary

Status: frozen for implementation planning, contract version 1.
Updated with maintainer decisions D-05 (30-day freshness window for live verification evidence) and D-09 (Option C), recorded in section 5.
Authoritative for: what counts as verification, how it is recorded, and what
may be claimed about Protocol 29.

Rule for everything below: nothing in this contract, and nothing built from it,
may invent a fixture, a verification date, an SDK version or a CAP behavior.
Unknown stays unknown.

## 1. Purpose and existing behavior

Verification is currently prose. `ProtocolCanary-Fixtures` records it in fixture
header comments and `docs/protocol-28.md` ("Verified live against
`https://soroban-testnet.stellar.org` on 2026-09-28 UTC with
`stellar-canary 0.1.1`"). `CONTRIBUTING.md` requires `source_reference` and the
`YYYY-MM-DD` UTC convention. `tools/validate/validate.py` checks structure only
(ids, enums, required fields, non-vague category). Nothing machine-readable says
when a fixture was last verified, against which network, or by what.

`protocol-27/` is intentionally empty by policy. The GitHub release
`protocol-28` has no assets and its text describes 5 fixtures; `main` now has 7.

## 2. Two different things called verification

| Kind | Meaning | Offline | What it proves |
|---|---|---|---|
| **Structural** | The fixture file conforms to the schema and the validator accepts it (`validate.py`, schema, tests). | Yes | The file is well formed. Says nothing about Stellar behavior. |
| **Source-checked** | A reviewer read the cited upstream source (CAP text, upstream XDR or release notes) and confirmed the assertion matches it. | Yes (human) | The assertion matches documentation at a reviewed date. |
| **Live read-only** | The fixture was executed against a real network endpoint using only read-only RPC calls. | No | The endpoint returned the asserted values at that time. |
| **Live simulation** | The fixture was executed with `simulateTransaction` on an unsigned transaction. | No | The simulation outcome at that time. |

A run's pass result is a result, not a verification of the fixture. Verification
is a separate, dated, attributed event.

## 3. Evidence record (`verificationVersion` 1)

Stored per fixture, in the registry (CF-02) or beside the fixture. Illustrative:

<!-- contract-example: verification-v1 -->
```json
{
  "verificationVersion": 1,
  "fixtureId": "example-rpc-fixture",
  "fixtureSha256": "0000000000000000000000000000000000000000000000000000000000000000",
  "method": "live-read-only",
  "checkedAt": "2000-01-01T00:00:00Z",
  "network": {
    "name": "testnet",
    "passphrase": "Test SDF Network ; September 2015",
    "observedProtocol": 0
  },
  "endpointHost": "rpc.example.invalid",
  "verifier": { "name": "stellar-canary", "version": "0.0.0" },
  "evidenceRef": "evidence/example.json",
  "outcome": "matched"
}
```

Rules:

- `fixtureSha256` is the CF-02 hash of the fixture file that was verified.
- `method` is one of `structural`, `source-checked`, `live-read-only`,
  `live-simulation`. There is no `unknown` method: absence of a record is the
  unknown state.
- `checkedAt` is RFC 3339 in UTC. The existing `YYYY-MM-DD` convention remains
  valid for header comments; the record needs the full instant.
- `network` is required for `live-*`. `observedProtocol` is copied from the real
  `getNetwork` response, never from the fixture's own `protocol`.
- `endpointHost` is a host name only: no path, query or token.
- `verifier.version` is the exact binary used. `evidenceRef` is a path inside the
  repository to the captured request and response, or null for `structural` and
  `source-checked`.
- `outcome` is `matched` or `mismatched`. A mismatched record is retained; it
  explains why a fixture is not trusted.
- A record is created by a person or a scheduled job that actually executed the
  step. Tooling must not generate one from a passing run unless that run was the
  verification step and its capture is saved.

## 4. Freshness states

Derived, never stored:

| State | Condition |
|---|---|
| `verified` | Latest record is `matched`, its `method` is one the fixture needs, and `checkedAt` is within the freshness window (approved 30 days for live methods under D-05). |
| `stale` | Latest matching record is older than the window. |
| `historical` | The only matching record observed a different `observedProtocol` than the fixture's `protocol`. |
| `mismatched` | Latest record is `mismatched`. |
| `unverified` | No record. |

Structural and source-checked records do not expire on a clock; they are
invalidated when the fixture file's `sha256` (CF-02) no longer equals the
record's `fixtureSha256`. A record whose `fixtureSha256` differs from the
current file is `historical`.

## 5. Protocol 29: what is established, and what is not

### Verified facts, with sources

1. **Upstream release information.**
   <https://developers.stellar.org/docs/networks/software-versions>, fetched
   2026-10-09: Protocol 29 is "a security-focused release that fixes
   vulnerabilities in Stellar Core. It introduces no new CAPs." Testnet
   2026-09-29, Mainnet 2026-10-01. The page lists XDR v28.0 and Rust `stellar-xdr`
   28.0.1 for both P29 networks, Soroban host 29.0.0, Stellar Core 29.0.0,
   Stellar RPC v29.0.0, Rust contract SDK 28.0.0, Stellar CLI 28.1.0, JS SDK
   v17.2.0 (Testnet) and v17.2.1 (Mainnet), RPC client v28.0.0.
2. **Network observation made while preparing this contract.** On
   2026-10-09 (about 07:20 UTC), read-only `getNetwork` returned `protocolVersion: 29` from
   `https://soroban-testnet.stellar.org` (passphrase `Test SDF Network ; September
   2015`) and from `https://mainnet.sorobanrpc.com` (passphrase `Public Global
   Stellar Network ; September 2015`). Command:
   `curl -s -X POST <url> -H 'Content-Type: application/json' -d
   '{"jsonrpc":"2.0","id":1,"method":"getNetwork"}'`. This is an observation
   made while preparing this contract. It is not a fixture verification, it has
   no record under section 3, and it must not be cited as one.
3. **Effect on the shipped Protocol 28 pack.** On 2026-10-09,
   `stellar-canary check --protocol 28` with this repository's built binary and
   the `ProtocolCanary-Fixtures` `main` pack against Testnet gave 5 pass, 2 fail:
   `p28-rpc-network` and `p28-rpc-latest-ledger` failed because they assert
   `protocolVersion = 28` and the endpoint reports 29. The tool printed
   `warning: the RPC endpoint reports protocol 29, but this run targets protocol
   28`. The Protocol 28 XDR and Soroban simulation fixtures passed.
4. **No fixtures exist for protocol 29.** `check --protocol 29` against the same
   pack skips all 7 fixtures. With `0.1.1` that exited 0 with `counts.total = 0`
   and `status: pass`; after D-02 (CF-01 section 7) it exits 2 and prints no
   report.

### Scope of any future "Protocol 29 compatibility" statement

Allowed to say, once a record exists for it:

- "This fixture was verified live read-only against <network> on <instant>
  and the endpoint reported protocol 29."
- "Protocol 29 introduced no new CAPs (upstream release page, <date fetched>),
  so no CAP-specific fixtures are planned."

Not allowed:

- Declaring Protocol 29 "supported" or "compatible" from a run that executed zero
  fixtures, or from Protocol 28 fixtures that declare `protocol = 28`.
- Reusing a Protocol 28 record as a Protocol 29 record.
- Naming any CAP behavior for Protocol 29.
- Stating SDK support beyond what the cited upstream page lists. Note that
  crates.io shows `soroban-sdk` 29.0.0 (published 2026-10-07) and `stellar-xdr`
  30.0.0 (published 2026-10-08) while the upstream page lists 28.0.0 and 28.0.1
  for Protocol 29. This project pins `stellar-xdr =28.0.1`. Which versions the
  engine should track is a maintainer decision and is not inferred here.

### Decision D-09: Option C (approved by the maintainer)

The Protocol 28 pack is preserved and Protocol 29 coverage is developed
separately, from real upstream specifications and real verification captures
only. The options that were considered:

| Option | Effect | Cost |
|---|---|---|
| A. Leave the Protocol 28 identity fixtures. A Protocol 28 target against a Protocol 29 network reports failure. | Honest and already the behavior. | Every Testnet CI run of the published pack fails. |
| B. Keep them but require an explicit network applicability field. | Failures become skips with a reason. | Needs engine support and a fixture format change. **Not chosen.** |
| **C. Keep the Protocol 28 pack unchanged and add Protocol 29 coverage separately.** | Clear separation. | Requires live evidence (section 3) before any Protocol 29 fixture exists. **Chosen.** |

Rules that follow from the decision:

1. **No Protocol 28 assertion is ever edited to expect protocol 29.**
   `p28-rpc-network` and `p28-rpc-latest-ledger` keep `protocolVersion = 28`. Their
   failure against a network that reports 29 is a correct result.
2. **Historical evidence stays.** Header comments, dated observations and
   `docs/protocol-28.md` in `ProtocolCanary-Fixtures` are not rewritten. A new
   observation is added beside them, with its own date.
3. **An endpoint that reports protocol 29 cannot serve as a Protocol 28 live
   environment.** As of 2026-10-09 both public networks checked report 29, so
   the live Protocol 28 RPC identity fixtures cannot pass against them. A
   Protocol 28 run against such an endpoint is expected to show that
   fixture failing, the mismatch warning, and the offline XDR fixtures passing.
   Running them against a Protocol 28 environment (for example a node the user
   operates) is the user's choice and is outside what this project provides.
   This is documented, not worked around.
4. **The mismatch stays visible.** The `warning: the RPC endpoint reports
   protocol N, but this run targets protocol M` line, `network.observedProtocol`
   and `targetProtocol` remain separate in the report (RH-05 pins this with
   tests). Observing a protocol is never reported as compatibility.
5. **Protocol 29 fixtures need all of:** an authoritative source (the upstream
   release page lists no CAPs for Protocol 29, so no CAP-specific fixture
   exists), a real capture made with a read-only call under section 3 with its
   `evidenceRef`, and a verification record. Until then there are no Protocol 29
   fixtures, and `check --protocol 29` against the shipped pack refuses to run
   (exit `2`, D-02) instead of reporting success.
6. **No new CAPs are invented for Protocol 29.** Anything described as
   "Protocol 29 behavior" must cite the upstream page or a capture.
7. **Out of scope throughout:** transaction submission, signing, private keys, a
   backend or a database.

#### Protocol 29 RPC coverage investigation

On 2026-10-09 between 08:38 and 08:39 UTC, one read-only request each of
`getNetwork`, `getLatestLedger`, `getVersionInfo`, `getHealth` and `getFeeStats`
was sent to `https://soroban-testnet.stellar.org` and to
`https://mainnet.sorobanrpc.com` (a public Mainnet endpoint whose operator this
project has not verified). All ten returned HTTP 200 with a JSON-RPC result.
Observed: `protocolVersion` was `29` in `getNetwork`, `getLatestLedger` and
`getVersionInfo` on both networks; `getVersionInfo` returned version
`29.0.0-b2b701685c79aee17fe4eb22dbd08a5dfd11594d` built `2026-09-22T14:52:44`
with `captiveCoreVersion` `stellar-core 29.0.0 (...)`; Mainnet `getNetwork` had
no `friendbotUrl`, Testnet's did. The `getLatestLedger` response carried a
`metadataXdr` field of about 1.28 million characters on Mainnet and 0.21
million on Testnet, which the engine downloads and discards.

What this establishes: the three methods the engine could assert on exist on
both networks and agree on protocol 29 at that moment. What it does not
establish: that any fixture is verified, that the endpoints are canonical, or
anything about behavior over time. It is an observation log, kept in
`ProtocolCanary-Fixtures` as `docs/protocol-29-rpc-observations.md`, and it is
not a verification record under section 3. The engine currently supports only
`getNetwork` and `getLatestLedger`; `getVersionInfo` support is specified in
CA-01.

## 6. Unsupported behavior

Fixtures that need Soroban host behavior the published SDK cannot construct stay
blocked and are listed, not stubbed. The one known case is CAP-0086 sparse-map
host functions (`ProtocolCanary-Fixtures` issue #2, blocked on upstream
`soroban-sdk`). A blocked item is recorded with its reason and the date the
reason was last checked.

## 7. Security and trust boundaries

Evidence files are data. They contain the captured JSON-RPC request and response
from a public endpoint and nothing else: no tokens, no private-endpoint URLs, no
secret headers. A verifier must refuse to write `endpointHost` values that carry
credentials, and must never submit a transaction.

## 8. Validation and test requirements

- Record schema validation (`verificationVersion` 1) in the Fixtures validator.
- A `live-*` record with `network` missing, `checkedAt` not UTC, or
  `endpointHost` containing `@`, `/` or `?` is invalid.
- Every record's `fixtureId` exists and its `fixtureSha256` is checked against
  the registry.
- A freshness computation test table covering all five states with an injected
  clock.
- A test that no tool path writes a record without an `evidenceRef` for
  `live-*` methods.

## 9. Cross-repository dependencies

CF-02 supplies `fixtureSha256`. CF-01 `results[].verification` copies the
record. CF-06 requires records for any pack described as "verified". CF-08
displays freshness and must show `unverified` plainly.

## 10. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CF-04-A Record schema and validator rules | ProtocolCanary-Fixtures | this contract, CF-02-C |
| CF-04-B Backfill structured records for existing Protocol 28 fixtures from their existing header comments, marking date precision honestly (header dates are days, not instants) | ProtocolCanary-Fixtures | CF-04-A |
| CF-04-C Freshness report generator (derived states) | ProtocolCanary-Fixtures | CF-04-A, CF-02-B |
| CF-04-D Engine parses and preserves the record on fixtures | Protocol-Canary | CF-04-A |
| CF-04-E Scheduled read-only verification job that saves captures | ProtocolCanary-Fixtures | CF-04-A. Needs maintainer approval of the schedule and endpoints. |
| CF-04-F Document the Protocol 29 boundary (section 5) in the Fixtures docs | ProtocolCanary-Fixtures | this contract |
| CF-04-G Engine regression tests for target 28 / observed 29, target 29 with no fixtures, target and observed equal, failed observation | Protocol-Canary | none |
| CF-04-H Any new Protocol 29 fixture | ProtocolCanary-Fixtures | **Blocked** until live evidence exists under section 3. D-09 (Option C) is decided; it does not unblock this. |
