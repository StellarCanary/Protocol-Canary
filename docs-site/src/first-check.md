# Your First Check

This walks through a real, historical run of Canary `v0.1.1` against the
Protocol 28 fixture pack, with real output from the pinned fixture
revision `e9d098df7931a9b94b183c6af807a58336b7b322` — nothing here is
illustrative text.

## 1. Install Canary

See [Installation](./installation.md):

```bash
cargo install --git https://github.com/StellarCanary/Protocol-Canary --tag v0.1.1 --locked
stellar-canary version
# stellar-canary 0.1.1
```

## 2. Clone the fixture pack

Clone the repository and check out the exact historical fixture commit
`e9d098df7931a9b94b183c6af807a58336b7b322` used for this run:

```bash
git clone https://github.com/StellarCanary/ProtocolCanary-Fixtures
git -C ProtocolCanary-Fixtures checkout e9d098df7931a9b94b183c6af807a58336b7b322
```

Pinning the fixture pack commit ensures the commands below reproduce the
exact five-result output shown in this guide. The `main` branch of
`ProtocolCanary-Fixtures` continues to add new fixtures over time, so an
unpinned checkout will produce different counts and checks.

## 3. Run the Protocol 28 check

```bash
stellar-canary check \
  --fixtures-dir ProtocolCanary-Fixtures/protocol-28 \
  --protocol 28
```

```text
Stellar Protocol Canary
────────────────────────────────────────

Project: Protocol-Canary (unknown)
Target protocol: 28
Network: testnet (observed protocol 28)

XDR
  3/3 PASS

RPC
  1/1 PASS

Soroban
  1/1 PASS

────────────────────────────────────────

5/5 applicable checks passed.

Status: PASS
```

## 4. Understand the PASS result

- `3/3` XDR checks — these ran entirely offline, verifying that CAP-0083
  and CAP-0085 XDR structures decode and round-trip byte-for-byte against
  the project's local dependencies. Offline XDR checks are strictly
  deterministic and require no network access.
- `1/1` RPC and `1/1` Soroban checks — these were live network observations
  recorded against `testnet` (`soroban-testnet.stellar.org`). The RPC and
  Soroban surfaces made live calls to the endpoint, which observed Protocol
  28, matching the target protocol. Unlike offline XDR checks, live network
  observations depend on remote endpoint availability and state at execution
  time.
- `Status: PASS` and exit code `0` — see [Exit Codes](./exit-codes.md) for
  the full contract.

This is **not** a claim of complete Protocol 28 coverage — see
[Protocol 28](./protocol-28.md) for exactly what these five fixtures do
and do not prove.

## 5. Run JSON mode

```bash
stellar-canary check \
  --fixtures-dir ProtocolCanary-Fixtures/protocol-28 \
  --protocol 28 \
  --json
```

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
    { "testId": "p28-xdr-cap83-empty-tx-set", "protocol": 28, "surface": "xdr", "status": "pass", "summary": "StellarValue round-tripped byte-for-byte", "durationMs": 0, "fixtureId": "p28-xdr-cap83-empty-tx-set" },
    { "testId": "p28-xdr-cap85-external-ref-malformed", "protocol": 28, "surface": "xdr", "status": "pass", "summary": "ContractExecutable was correctly rejected", "details": "failed to fill whole buffer", "durationMs": 0, "fixtureId": "p28-xdr-cap85-external-ref-malformed" },
    { "testId": "p28-xdr-cap85-external-ref-roundtrip", "protocol": 28, "surface": "xdr", "status": "pass", "summary": "ContractExecutable round-tripped byte-for-byte", "durationMs": 0, "fixtureId": "p28-xdr-cap85-external-ref-roundtrip" },
    { "testId": "p28-rpc-network", "protocol": 28, "surface": "rpc", "status": "pass", "summary": "GetNetwork response matched all 3 assertion(s)", "durationMs": 574, "fixtureId": "p28-rpc-network" },
    { "testId": "p28-soroban-native-asset-name", "protocol": 28, "surface": "soroban", "status": "pass", "summary": "simulation succeeded as expected", "durationMs": 380, "fixtureId": "p28-soroban-native-asset-name" }
  ],
  "git": { "commit": "ffa072cb14c682bfa5ebf158ab9bbe6058962aef", "branch": "main", "isDirty": false }
}
```

This is a real, unedited historical run captured with Canary `0.1.1` and
fixture revision `e9d098df7931a9b94b183c6af807a58336b7b322` against
`soroban-testnet.stellar.org`. The XDR results were evaluated completely
offline, while the RPC and Soroban entries record live network responses
observed during that historical run. Field-by-field meaning is in
[JSON Report](./json-report.md).

## 6. Understand the exit code

```bash
echo $?
# 0
```

`0` means every applicable fixture passed. See [Exit Codes](./exit-codes.md)
for the full table (`0`–`5`) and what each one means.

## What's next

- [CLI Reference](./cli/overview.md) for every command and flag.
- [Protocol 28](./protocol-28.md) for exactly what these five fixtures do
  and do not cover.
- [GitHub Action](./github-action.md) to run this in CI instead of locally.
