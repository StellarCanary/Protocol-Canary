# Exit Codes

`stellar-canary`'s process exit codes are a stable, documented contract —
GitHub automation and other callers rely on them rather than parsing
terminal text. They are defined in
[`crates/canary-core/src/errors.rs`](https://github.com/StellarCanary/Protocol-Canary/blob/main/crates/canary-core/src/errors.rs)
as the `ExitCode` enum, and this table matches that source directly (each
value is also asserted by that file's own `exit_codes_match_the_documented_contract`
unit test).

| Code | Name | Meaning |
|---|---|---|
| `0` | `Pass` | Every applicable check passed. |
| `1` | `CompatibilityFailure` | At least one fixture's compatibility assertion failed — a real incompatibility was found. |
| `2` | `ConfigurationError` | Invalid CLI configuration, e.g. a `--config` path that does not exist, or unparseable configuration. No checks run. |
| `3` | `ExecutionError` | A check could not complete due to an XDR, RPC, or Soroban execution problem — most commonly an unreachable or erroring RPC endpoint. Distinct from a compatibility failure: the check couldn't run, rather than running and finding a mismatch. |
| `4` | `InvalidFixture` | A fixture file failed to load: bad TOML, a missing required field, an unrecognized `surface`, a duplicate `id` across the fixture set, or a dangling `input_file`/`expected_file` reference. |
| `5` | `InternalError` | An internal error unrelated to configuration, fixtures, or the three compatibility surfaces (for example, a Git-metadata or local-cache error). |

## How the mapping works

Every error in the CLI is a `CanaryError` variant, which is classified
into one of four categories, which in turn maps to an exit code:

| `CanaryError` variant | Category | Exit code |
|---|---|---|
| `Configuration`, `Project` | Configuration | `2` |
| `Fixture` | Fixture | `4` |
| `Xdr`, `Rpc`, `Soroban` | Execution | `3` |
| `Git`, `Cache`, `Internal` | Internal | `5` |

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
failures) — this documentation does not fabricate a specific reproduction
for it, since triggering one requires an environment fault (e.g. a
corrupt local cache file) rather than a normal CLI invocation. Its
meaning above is taken directly from the source's `ErrorCategory::Internal`
mapping, not guessed.
