# Exit Codes

`stellar-canary`'s process exit codes are a stable, documented contract —
GitHub automation and other callers rely on them rather than parsing
terminal text. They are defined in
[`crates/canary-core/src/errors.rs`](https://github.com/StellarCanary/Protocol-Canary/blob/main/crates/canary-core/src/errors.rs)
as the `ExitCode` enum, and this page matches that source directly: the
`exit_code_docs` integration tests in `canary-cli` parse this file and
compare every table row and section heading against `ExitCode::ALL`, so a
new or renumbered exit code fails the test suite until this page is
updated (each value is also asserted by that source file's own
`exit_codes_match_the_documented_contract` unit test).

| Code | Name | Meaning |
|---|---|---|
| `0` | `Pass` | Every applicable check passed. |
| `1` | `CompatibilityFailure` | At least one fixture's compatibility assertion failed — a real incompatibility was found. |
| `2` | `ConfigurationError` | The invocation was unusable or no check applied, so no checks ran. |
| `3` | `ExecutionError` | At least one check could not complete — it was executed but hit an XDR, RPC, or Soroban execution problem. |
| `4` | `InvalidFixture` | A fixture file failed to load or validate, so the run never started. |
| `5` | `InternalError` | An internal error unrelated to configuration, fixtures, or the three compatibility surfaces. |

## Branching on the exit status

```sh
stellar-canary check --fixtures-dir ./fixtures
case $? in
  0) echo "protocol-compatible" ;;
  1) echo "incompatible: a check ran and failed (see report above)" ;;
  2) echo "configuration error: no checks ran (see stderr)" >&2; exit 1 ;;
  3) echo "execution error: checks could not complete (RPC down?)" >&2; exit 1 ;;
  4) echo "invalid fixture: fix the fixture files first" >&2; exit 1 ;;
  *) echo "internal error" >&2; exit 1 ;;
esac
```

The distinction CI usually cares about is `1` versus everything else:
exit `1` is *compatibility evidence* (a check ran and the surfaces
disagreed), while exits `2`–`5` mean the run itself needs attention and
prove nothing about compatibility either way.

## `0` — `Pass`

**When it happens.** Every applicable check passed. Concretely: `check`
finishes with a `pass` (or `warning` — warnings do not fail a run unless
`warnings_are_failures` is enabled in the policy) and no result recorded
an execution error; `inspect`, `fixtures`, `report`, `cache clear`, and
`version` complete successfully; `config validate` finds the project's
`.stellar-canary.toml` valid (or no file at all, which is valid: the
built-in defaults apply); and `check --allow-empty` exits `0` on
a run in which no fixture applies.

**Example.** A nightly job runs `stellar-canary check` against the
Protocol 28 fixture pack after every merge; exit `0` is the "still
compatible, keep shipping" signal that lets the next stage publish the
release.

## `1` — `CompatibilityFailure`

**When it happens.** At least one fixture's compatibility assertion
failed: the check *ran* and found a real mismatch (for example an XDR
value that no longer round-trips, or an RPC endpoint that no longer
reports the expected protocol). A `warning` result also becomes a
failure when the policy sets `warnings_are_failures = true`. The run
completed — exit `1` is evidence, not an outage.

The read-only verdict commands use the same number for the same reason:
`lock verify` exits `1` when the pin does not hold and `config validate`
exits `1` when `.stellar-canary.toml` is not a valid configuration. In
each case the question the caller asked was answered, and the answer was
no — which is why neither is a `2`, reserved for a call that could not be
answered at all.

**Example.** The configured RPC endpoint upgrades to protocol 29 while
the pack still asserts protocol 28: the identity fixture fails, `check`
exits `1`, and CI blocks the release until the fixtures (or the
dependency) are updated.

## `2` — `ConfigurationError`

**When it happens.** No checks ran because the invocation itself was
unusable:

- `--config` names a file that cannot be read or parsed;
- `.stellar-canary.toml` contains an unknown section or key (config is
  parsed strictly);
- `--protocol 0` or another invalid protocol value;
- `--fixture`/`--surface` filters that match no fixture;
- the configured network passphrase does not match the one the endpoint
  reports;
- `--output` points at a path that cannot be written;
- no fixture applies to the run — missing or empty fixtures directory,
  or every fixture skipped by protocol, disabled surface, or missing
  project capability — and `--allow-empty` was not given (since `0.2.0`).

Argument-parser usage errors (an unknown subcommand or flag) also exit
`2`: an unusable command line is a configuration problem too.

**Example.** A CI job passes `--protocol 29` against a pack that only
contains Protocol 28 fixtures. Instead of the pre-`0.2.0` vacuous exit
`0`, the run stops before executing anything and exits `2`, forcing the
pipeline to notice that nothing was actually checked.

## `3` — `ExecutionError`

**When it happens.** A check was attempted but could not complete — most
commonly an unreachable or erroring RPC endpoint, but also an XDR or
Soroban execution problem. Any result with status `error` overrides the
pass/fail counts: a run that could not fully execute must never be
reported as a clean pass. Distinct from exit `1`: here the check
*couldn't* run, rather than running and finding a mismatch.

**Example.** A transient Stellar RPC outage lands mid-nightly: the XDR
checks still run and pass, the RPC fixture records an execution error,
and `check` exits `3`. The CI step retries or pages — it does not mark
the protocol incompatible.

## `4` — `InvalidFixture`

**When it happens.** A fixture file failed to load or validate, so the
run never started: bad TOML, a missing required field, an unknown key
(fixture bodies are parsed strictly), an unrecognized `surface`, a
duplicate `id` across the fixture set, a dangling
`input_file`/`expected_file` reference, or a symbolic link, junction, or
unsafe path under the fixtures directory.

**Example.** A contributor adds a fixture but misspells `description`
as `descrption`: `check` exits `4` with the file, line, and missing
field, and the review bot fails the pull request before the fixture
pack ships broken.

## `5` — `InternalError`

**When it happens.** An error outside the configuration, fixture, and
execution categories: a Git-metadata or local-cache failure, or (for
`inspect`) a working directory that cannot be read. This page does not
fabricate a reproduction for it — triggering one requires an
environment fault (e.g. a corrupt local cache file) rather than a normal
CLI invocation. Its meaning is taken directly from the source's
`ErrorCategory::Internal` mapping, not guessed.

**Example.** A corrupt entry in `.stellar-canary-cache/` makes the run
abort before executing checks; `check` exits `5`, and the fix is to
delete the cache directory (it is safe to remove) and re-run.

## How the mapping works

Every error in the CLI is a `CanaryError` variant, which is classified
into one of four categories, which in turn maps to an exit code:

| `CanaryError` variant | Category | Exit code |
|---|---|---|
| `Configuration`, `Project` | Configuration | `2` |
| `Fixture` | Fixture | `4` |
| `Xdr`, `Rpc`, `Soroban` | Execution | `3` |
| `Git`, `Cache`, `Internal` | Internal | `5` |

A `check` run that completes does not flow through `CanaryError` at all:
`canary_core::exit_code_for_run` turns the run's results and policy
decision into the exit code — `3` if any result recorded an execution
error (overriding everything else), `1` for a `fail` decision, and `0`
for `pass` or `warning`. Only `main.rs` terminates the process, and it
does so with `ExitCode::code()` — no exit status is produced anywhere
else, which the `exit_code_docs` tests also enforce for
`crates/canary-cli/src`.

## Verified examples

Each of these was captured from a real run of the `v0.1.1` binary, not
written from memory:

**Exit `0`** — `check` against the real Protocol 28 pack: `5/5 applicable
checks passed.`

**Exit `1`** — `check` against a fixture with a deliberately wrong
expected value:

```text
error: (terminal output shows "❌ FAIL" and "Status: NOT READY";
the machine-readable status is "fail" — see JSON Report)
```

**Exit `2`** — `check --config /does/not/exist.toml`:

```text
error: configuration error: failed to read configuration file /does/not/exist.toml: No such file or directory (os error 2)
```

**Exit `3`** — `check` with `--rpc-url` pointed at an unreachable host:

```text
Failure:
p28-rpc-network

failed to call GetNetwork

network transport error calling getNetwork: error sending request for url (...)
```

Note that in this case the XDR checks still ran and passed (`3/3 PASS`)
— an RPC outage does not block offline checks. `status` in JSON output is
`"error"`, which overrides whatever the pass/fail counts alone would
otherwise suggest.

**Exit `4`** — `check` against a fixture missing a required field:

```text
error: invalid fixture: failed to parse fixture file /path/bad.toml: TOML parse error at line 1, column 1
  |
1 | id = "demo-missing-field"
  | ^^^^^^^^^^^^^^^^^^^^^^^^^
missing field `description`
```

**Exit `5`** is reserved for internal errors (Git metadata or local-cache
failures) — see [when it happens](#5--internalerror) above.
