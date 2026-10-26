# `config`

Works with the project configuration file. `config validate` checks
`.stellar-canary.toml` for syntax, table structure and value types — and for
the rules the loader enforces after parsing — without loading a fixture,
running a check, or opening a network socket (issue #457).

```text
Validate the project configuration without running any check

Usage: stellar-canary config <COMMAND>

Commands:
  validate  Check .stellar-canary.toml without running any compatibility check
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `config validate`

```bash
stellar-canary config validate
```

```text
Valid: /path/to/project/.stellar-canary.toml
  version                      = 1
  protocol                     = 28
  network                      = (not set: defaults to testnet)
  rpc_url                      = (not set)
  fixtures_dir                 = (not set: defaults to fixtures)
  project.type                 = auto
  tests                        = xdr=true rpc=true soroban=true
  policy.warnings_are_failures = false
```

The file is parsed by the same strict loader `check` uses, so a
configuration this command accepts is a configuration `check` reads
identically. Unknown keys and unknown tables are rejected rather than
ignored — that is what catches a misspelled `[tests]` — and so is a value of
the wrong type.

A key the file leaves unset is reported as not set, together with the
built-in default that then applies. `--protocol`, `--network`,
`--rpc-url` and `--fixtures-dir` still override the file at run time;
this report describes the file, not a run.

### Errors point at the line

Every failure names the offending field and where it is, because the TOML
parser's own diagnostic is passed through:

```bash
stellar-canary config validate
```

```text
error: failed to parse configuration file /path/to/project/.stellar-canary.toml: TOML parse error at line 4, column 2
  |
4 | [test]
  |  ^^^^
unknown field `test`, expected one of `version`, `protocol`, `network`, `rpc_url`, `fixtures_dir`, `project`, `tests`, `policy`
```

Fix the line, re-run. The parser reports the first problem it finds.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | The configuration is valid — or there is no `.stellar-canary.toml`, in which case the project runs on the built-in defaults, which are valid. |
| `1` | The file is not a valid configuration. This is the answer to the question that was asked, not a broken invocation. |
| `2` | Validation could not start: `--project-root` does not resolve, or the file named by `--config` cannot be read. |

See [Exit Codes](../exit-codes.md).

### Options

| Flag | Meaning |
|---|---|
| `--project-root <PATH>` | Treat another directory as the project root. The root is never searched for in parent directories. |
| `--config <PATH>` | Validate a specific file instead of `.stellar-canary.toml` in the project root. A relative path resolves against `--project-root`. Unlike the default location, a file named explicitly must exist: not being able to read it is exit `2`. |

```bash
# A monorepo member, from the repository root.
stellar-canary config validate --project-root services/consumer

# A CI copy of the configuration, with no project root implied.
stellar-canary config validate --config ci/stellar-canary.toml
```

## Typical use

Run it as the first step of a job, so a typo in the configuration fails
before fixtures are fetched and a run is spent:

```bash
stellar-canary config validate || exit 1
stellar-canary check --offline --format json --output report.json
```

Nothing is written and no network call is made, so the check is safe to run
in a pre-commit hook:

```bash
stellar-canary config validate --project-root "$(git rev-parse --show-toplevel)"
```

## Schema

The configuration shape is published as
[`config-v1.schema.json`](https://github.com/StellarCanary/Protocol-Canary/blob/main/docs/schemas/config-v1.schema.json),
a draft 2020-12 JSON Schema you can point an editor or a CI job at.
`crates/canary-config/tests/schema.rs` tests that file against the documented
shapes and the rejections version 1 states;
`crates/canary-config/tests/schema_parity.rs` tests it against this loader.

The two sides are deliberately not identical at one point. The schema also
constrains the shape of `network`, `rpc_url` and `fixtures_dir` — a name is one
token, an endpoint is an absolute `http` or `https` URL, a fixture directory is
a path relative to the project root — while the loader stores those three values
verbatim, so that a bad stored value cannot stop `--network`, `--rpc-url` or
`--fixtures-dir` from outranking it. The schema is therefore the stricter side
for exactly those documents, and that set is written down in the parity suite
rather than left to chance: nothing the schema accepts is something the loader
refuses, and every place the schema refuses more is listed, with its reason, and
re-tested.
