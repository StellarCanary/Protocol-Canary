# `check`

Runs compatibility checks against the current project. This is the
command everything else in this documentation builds toward.

```text
Run compatibility checks against the current project

Usage: stellar-canary check [OPTIONS]

Options:
      --protocol <PROTOCOL>          Target protocol version (overrides configuration)
      --network <NETWORK>            Network to run live checks against [default: testnet]
      --rpc-url <RPC_URL>            RPC endpoint to use for live checks
      --config <CONFIG>              Path to a configuration file (default: .stellar-canary.toml in the project root)
      --fixtures-dir <FIXTURES_DIR>  Directory containing fixture files [default: fixtures]
      --format <FORMAT>              Output format [default: terminal] [possible values: terminal, json, markdown]
      --json                         Shorthand for --format json
      --verbose
      --quiet
  -h, --help                         Print help
```

## Protocol selection

Precedence, highest first: `--protocol` flag, then `.stellar-canary.toml`'s
`protocol` field, then the built-in default of `28`.

## Fixture directory

`--fixtures-dir` defaults to `fixtures` relative to the current directory.
It accepts any local path — including a checkout of
[`ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures)
— and is loaded recursively. A directory that does not exist is treated as
zero fixtures (a trivial `0/0` pass), not an error. See [Fixtures](../fixtures-guide.md).

## Output formats

- `terminal` (default) — human-readable, colorized when the output is a
  TTY.
- `--format json` (or the `--json` shorthand) — machine-readable, the
  contract [`ProtocolCanary-Action`](../github-action.md) consumes. See
  [JSON Report](../json-report.md).
- `--format markdown` — a Markdown table, suitable for a PR comment or job
  summary.

The report is printed to stdout **regardless of exit code**, including on
a compatibility failure — a caller does not need to inspect stderr to get
the report.

## Network behavior

Not every check requires the network:

- **XDR checks are offline.** They decode/encode against the official
  `stellar-xdr` crate with no network call.
- **RPC and Soroban checks require a live, reachable `--rpc-url`.**
  `--network` selects which network name is reported (`testnet` by
  default); `--rpc-url` is the actual endpoint contacted. If RPC/Soroban
  checks are enabled (the default) and the endpoint is unreachable, that
  is reported as an **execution error**, not skipped — see
  [Exit Codes](../exit-codes.md) and
  [network troubleshooting](../troubleshooting.md#network-troubleshooting).

Disable a surface in `.stellar-canary.toml` (`[tests] rpc = false` /
`soroban = false`) to run fully offline.

## Example: PASS

```bash
stellar-canary check --fixtures-dir ProtocolCanary-Fixtures/protocol-28 --protocol 28
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

Exit code `0`.

## Example: JSON

```bash
stellar-canary check --fixtures-dir ProtocolCanary-Fixtures/protocol-28 --protocol 28 --json
```

See the full real example in [Your First Check](../first-check.md#5-run-json-mode)
and the field-by-field reference in [JSON Report](../json-report.md).

## Example: FAIL

A real fixture deliberately given a wrong expected value:

```text
Stellar Protocol Canary
────────────────────────────────────────

Project: Protocol-Canary (unknown)
Target protocol: 28
Network: testnet (observed protocol 28)

XDR
  demo-xdr-fail                ❌ FAIL

────────────────────────────────────────

0/1 applicable checks passed.

Status: NOT READY

Failure:
demo-xdr-fail

failed to decode StellarValue input

failed to fill whole buffer
```

Exit code `1`. Note that the terminal reporter's headline for a failing
run reads `Status: NOT READY` — the machine-readable `status` field in
`--json` output is `"fail"`; see [JSON Report](../json-report.md) for the
exact string values a script should key off of instead of terminal text.
