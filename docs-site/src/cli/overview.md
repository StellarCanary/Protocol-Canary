# CLI Reference

`stellar-canary` ships ten subcommands. This reference matches the
shipped CLI, verified directly against the binary's help output.

```text
Rehearse Stellar protocol upgrades before they reach your production stack.

Usage: stellar-canary <COMMAND>

Commands:
  check        Run compatibility checks against the current project
  inspect      Print offline project diagnostics
  fixtures     List fixtures available for a protocol
  report       Render a previously generated JSON report
  diff         Compare two stored JSON reports and render the diff (CF-05)
  cache        Manage the local result cache
  config       Validate the project configuration without running any check
  lock         Manage the fixture-pack lockfile: bare `lock` writes the pin (CF-03-B), `lock verify` re-checks it (CF-03-D)
  completions  Print a shell completion script
  version      Print the tool version
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help (see more with '--help')
  -V, --version  Print version
```

| Command | Purpose |
|---|---|
| [`cache`](./cache.md) | Manage the local result cache — `cache clear` deletes cached fixture execution results and reports reclaimed disk space. |
| [`check`](./check.md) | Run compatibility checks against the current project. The main command. |
| [`config`](./config.md) | Validate `.stellar-canary.toml` — syntax, tables, types and loader rules — without running any check. |
| [`inspect`](./inspect.md) | Print offline project diagnostics — no fixtures run, no network call. |
| [`fixtures`](./fixtures.md) | List the fixtures a given `--fixtures-dir`/`--protocol` combination would run. |
| [`report`](./report.md) | Re-render a previously saved JSON report in another format, without touching the network. |
| [`diff`](./diff.md) | Compare two saved JSON reports and report the verdict, gating on the exit code. |
| [`lock`](./lock.md) | Pin the fixture pack (`lock`) and re-check the pin (`lock verify`). |
| [`completions`](./completions.md) | Print a shell completion script for bash, zsh, fish or powershell. |
| `version` | Print the tool's own version. See [version](./version.md). |

## Project root

`check`, `inspect`, `fixtures`, `lock` and `config` work on the current
directory by default.
`--project-root <PATH>` names a different directory instead, for example one
member of a monorepo. The path must exist and be a directory; otherwise the
command exits with code `2`.

With `--project-root`, a relative `--config`, `--fixtures-dir`, `--cache-dir`
or `--output` is resolved against that directory, not against where the
command was run.
The engine never searches parent directories for a project root, so a
manifest in a parent directory is never picked up.

Every command accepts `-h`/`--help` for the exact, current flag list. If
this page and your installed binary's `--help` output ever disagree,
trust `--help` — it is generated directly from the CLI's own argument
parser (`clap`) and cannot drift from the shipped behavior the way a
document can.
