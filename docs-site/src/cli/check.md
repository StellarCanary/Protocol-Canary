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
      --rpc-timeout <RPC_TIMEOUT>    Request timeout for the RPC client in seconds [default: 10]
      --fixtures-dir <FIXTURES_DIR>  Directory containing fixture files [default: fixtures]
      --format <FORMAT>              Output format [default: terminal] [possible values: terminal, json, markdown]
      --json                         Shorthand for --format json
      --output <PATH>                Write the rendered report to this path, in addition to stdout.
      --verbose                      Include skip reasons in Markdown/terminal output and populate the JSON report's verbose field.
      --quiet                        Shorten terminal-format output to a single status line.
      --max-concurrency <MAX_CONCURRENCY>  Maximum number of concurrent network requests for RPC/Soroban fixtures [default: 4]
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

> **Note on verbosity:** The `--quiet` flag only affects the `terminal` output format, condensing the output to a single `Status:` line instead of a full report. The `--verbose` flag affects the detail included: it adds skip reasons to both `markdown` and `terminal` outputs, and is passed through into the `json` report's `verbose` field.

The report is printed to stdout **regardless of exit code**, including on
a compatibility failure — a caller does not need to inspect stderr to get
the report.

## Writing the report to a file

`--output <PATH>` writes the rendered report to `PATH` **in addition to**
printing it to stdout. Stdout behavior is unchanged, so an existing shell
redirection (`stellar-canary check --json > result.json`) still works, and
a caller that cannot capture stdout — for example one wrapped by a tool
that does not pass stdout through cleanly — has a direct way to get the
report into a file.

The file receives exactly the bytes stdout receives, in the format
selected by `--format`/`--json`, and including the trailing newline
`println!` adds. That also means `--quiet` writes its single `Status:`
line rather than a full report. The file is created if `PATH` does not
exist and truncated if it does, and it is written before anything is
printed, so an unusable path fails before a report reaches stdout.

```bash
stellar-canary check --json --output result.json
stellar-canary report result.json --format markdown
```

The exit code for the run itself is unchanged by `--output`: it never
turns a `0` into a `1` or the other way around. The one failure mode the
flag adds is `PATH` itself — a directory that does not exist, or one the
process cannot write — which is reported as a **configuration error**
(exit code `2`) and prints nothing to stdout, since the report could not
be produced where it was asked for. See [Exit Codes](../exit-codes.md).

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
- **The endpoint's identity is validated against the run's assumptions.**
  When `getNetwork` succeeds, its passphrase is compared to `--network`
  and its protocol version to `--protocol`: a passphrase mismatch aborts
  as a **configuration error** (exit `2`) before any check runs, since
  results from the wrong network must not be attributed to the requested
  one; an observed protocol that differs from the target prints a
  `warning:` line on stderr (e.g. `warning: the RPC endpoint reports
  protocol 27, but this run targets protocol 28`) and the run continues —
  observing a not-yet-upgraded network while rehearsing the next protocol
  is a legitimate use case, but it must be visible rather than only an
  `(observed protocol N)` annotation in the report.

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
