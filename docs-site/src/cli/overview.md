# CLI Reference

`stellar-canary` ships five subcommands. This reference matches the CLI
shipped in `v0.1.1`, verified directly against `stellar-canary --help`
and each subcommand's own `--help` output.

```text
Rehearse Stellar protocol upgrades before they reach your production stack.

Usage: stellar-canary <COMMAND>

Commands:
  check     Run compatibility checks against the current project
  inspect   Print offline project diagnostics
  fixtures  List fixtures available for a protocol
  report    Render a previously generated JSON report
  version   Print the tool version
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

| Command | Purpose |
|---|---|
| [`check`](./check.md) | Run compatibility checks against the current project. The main command. |
| [`inspect`](./inspect.md) | Print offline project diagnostics — no fixtures run, no network call. |
| [`fixtures`](./fixtures.md) | List the fixtures a given `--fixtures-dir`/`--protocol` combination would run. |
| [`report`](./report.md) | Re-render a previously saved JSON report in another format, without touching the network. |
| `version` | Print the tool's own version. See [version](./version.md). |

Every command accepts `-h`/`--help` for the exact, current flag list. If
this page and your installed binary's `--help` output ever disagree,
trust `--help` — it is generated directly from the CLI's own argument
parser (`clap`) and cannot drift from the shipped behavior the way a
document can.
