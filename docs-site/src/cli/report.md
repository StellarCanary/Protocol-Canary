# `report`

Renders a previously generated JSON report in another format, without
touching the network.

```text
Render a previously generated JSON report

Usage: stellar-canary report [OPTIONS] <PATH>

Arguments:
  <PATH>  Path to a JSON report produced by `stellar-canary check --json`

Options:
      --format <FORMAT>  Output format to render the stored report as [default: markdown] [possible values: terminal, json, markdown]
  -h, --help             Print help
```

This is a pure re-render: it parses the exact shape documented in
[JSON Report](../json-report.md) (`JsonReporter::parse`) and reproduces it
in the requested format — it never re-runs a check or makes a network
call. Note the default output format here is `markdown`, unlike `check`'s
default of `terminal`.

## Example

```bash
stellar-canary check --fixtures-dir ProtocolCanary-Fixtures/protocol-28 --protocol 28 --json > result.json
stellar-canary report result.json --format markdown
```

```text
## Stellar Protocol Canary

Protocol 28 compatibility

| Surface | Result |
|---|---|
| XDR | ✅ Pass |
| RPC | ✅ Pass |
| Soroban | ✅ Pass |

**Result: PASS**
```

```bash
stellar-canary report result.json --format terminal
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

There is no export format beyond `terminal`/`json`/`markdown` — do not
assume an HTML, PDF, or CSV output exists; it does not.
