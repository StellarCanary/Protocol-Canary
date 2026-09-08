# Troubleshooting

## `stellar-canary: command not found`

**Symptom.** Shell reports the binary doesn't exist after `cargo install`.

**Likely cause.** `~/.cargo/bin` isn't on your `PATH`, or the install
targeted a different Cargo home.

**What to check.** `cargo install --list | grep canary-cli` and
`echo $PATH`.

**Fix.** Add `~/.cargo/bin` (or your `$CARGO_HOME/bin`) to `PATH`, or
invoke the binary by its full path.

## Wrong version reported

**Symptom.** `stellar-canary version` prints something other than
`0.1.1`, or behavior doesn't match this documentation.

**Likely cause.** An older install (e.g. `0.1.0`) is still first on
`PATH`, or `cargo install` was run without `--tag v0.1.1`.

**What to check.** `stellar-canary version` and `which stellar-canary`.

**Fix.** Reinstall with the exact tag:
`cargo install --git https://github.com/StellarCanary/Protocol-Canary --tag v0.1.1 --locked --force`.
`0.1.0` predates `ContractExecutable` XDR support two current Protocol 28
fixtures require, and its report predates the `counts` field — see
[Releases](./releases.md).

## Fixture directory not found

**Symptom.** `check`/`fixtures` reports `0/0 applicable checks` when you
expected fixtures to run.

**Likely cause.** `--fixtures-dir` is missing, misspelled, or relative to
the wrong working directory — a nonexistent `--fixtures-dir` is treated as
zero fixtures, not an error, by design.

**What to check.** `stellar-canary fixtures --fixtures-dir <path> --protocol <N>`
first, to see exactly what would be loaded, before running `check`.

**Fix.** Correct the path, or confirm you're in the directory you think
you're in.

## Malformed fixture

**Symptom.** `check`/`fixtures` exits with code `4` and a TOML parse
error or "missing field" message.

**Likely cause.** A required field is missing (every fixture needs `id`,
`protocol`, `surface`, `category`, `description`), a surface-specific
required field is missing (see [Fixtures](./fixtures-guide.md#per-surface-body-fields)),
or the TOML itself doesn't parse.

**What to check.** The exact file path in the error message, and the
schema table in [Fixtures](./fixtures-guide.md#schema).

**Fix.** Correct the file. No fixtures from that load run until it's
fixed — this is deliberate, not a partial-load bug.

## Protocol mismatch

**Symptom.** A fixture you expect to run is missing from
`stellar-canary fixtures` output, or shows up in `skipped` in the JSON
report.

**Likely cause.** The fixture's declared `protocol` doesn't match the
run's target protocol (`--protocol`, or `.stellar-canary.toml`'s
`protocol` field, or the default of `28`).

**What to check.** `skipped[].reason` in `--json` output, e.g.
`"fixture targets protocol 27, this run targets protocol 28"`.

**Fix.** Pass `--protocol` matching the fixture, or confirm you meant to
target a different protocol than your fixture pack provides.

## RPC unreachable

**Symptom.** `check` exits `3`, `status` is `error`, and RPC/Soroban
fixtures report a transport error.

**Likely cause.** The configured `--rpc-url` is wrong, the endpoint is
down, or there's no network access from the current machine/runner.

**What to check.** The exact error text (e.g. `error sending request for
url (...)`), and whether XDR-only fixtures in the same run still passed
(they should — see [Network Troubleshooting](#network-troubleshooting)
below).

**Fix.** Correct `--rpc-url`/`--network`, confirm connectivity, or
disable `rpc`/`soroban` checks in `.stellar-canary.toml` if you only need
the offline XDR surface.

## Soroban simulation failure

**Symptom.** A Soroban fixture reports `fail` or `error`.

**Likely cause.** Either a genuine simulation-behavior mismatch (`fail` —
the fixture's `expect.kind` didn't match what `simulateTransaction`
actually returned), or a transport/execution problem reaching the
network (`error`).

**What to check.** `results[].status` (`fail` vs. `error`) and
`results[].details` in the JSON report — they are reported differently on
purpose; see [JSON Report](./json-report.md).

**Fix.** For `fail`: re-verify the fixture's expected behavior against
the real network and its own header comment. For `error`: treat it as a
network problem — see RPC unreachable above.

## GitHub Action failure

**Symptom.** The workflow step running `ProtocolCanary-Action` fails.

**Likely cause.** Either a real compatibility failure (`status: fail`) or
an execution failure (`status: execution-failed` — Canary couldn't
install, run, or produce parseable output).

**What to check.** The job summary text: a compatibility failure names
the specific failing `testId`(s); an execution failure says "Protocol
Canary could not be executed" with the actual diagnostic. See
[CI Workflows & Failure Behavior](./ci-workflows.md#two-different-kinds-of-red).

**Fix.** For a compatibility failure, fix the underlying incompatibility.
For an execution failure, check the diagnostic — commonly a missing Rust
toolchain issue on a non-standard runner, a bad `version` input, or a
`timeout-minutes` that's too short for the runner's cold-cache build time.

## Action version/reference problem

**Symptom.** The Action can't be resolved, or installs an unexpected
`Protocol-Canary` version.

**Likely cause.** `uses:` references a nonexistent tag, or the `version`
input doesn't match a real `Protocol-Canary` release tag.

**What to check.** [Releases](./releases.md) for the currently verified
combination (`Action @v1` installing `Protocol-Canary 0.1.1`).

**Fix.** Use `uses: StellarCanary/ProtocolCanary-Action@v1` and either
omit `version` (defaults to `0.1.1`) or set it to a real release tag
without a leading `v`.

## JSON parsing problem

**Symptom.** A script consuming `--json` output fails to parse a field it
expected.

**Likely cause.** Assuming a field is always present when it's actually
conditional — `network` (absent for offline runs), `skipped` (absent when
empty), or `results[].details` (absent, not `null`, when there's nothing
to add).

**What to check.** The "Always present?" column in
[JSON Report](./json-report.md#field-reference).

**Fix.** Guard for absence rather than assuming presence; treat "missing
`counts`" as "derive it from `results`" rather than an error, matching
what the Action itself does.

## Exit code interpretation

See the dedicated [Exit Codes](./exit-codes.md) reference — do not guess
at a code's meaning from a partial memory of it; the source
(`crates/canary-core/src/errors.rs`) and that page are the two places to
check.

## Network Troubleshooting

Some compatibility checks (RPC, Soroban) genuinely require a live,
reachable Stellar RPC endpoint — Protocol Canary does not claim to be
fully offline; only the XDR surface is. This means **a failure to reach
RPC is a different kind of result than a real protocol compatibility
failure**, and the tool reports them differently on purpose:

| | RPC/network failure | Compatibility failure |
|---|---|---|
| Cause | Endpoint unreachable, DNS failure, timeout | The assertion ran and genuinely didn't match |
| `status` | `error` | `fail` |
| Exit code | `3` | `1` |
| Offline (XDR) fixtures in the same run | Unaffected — still run and report their real result | Unaffected |
| Terminal marker | `‼ ERROR` with a transport error message | `❌ FAIL` with the assertion's own failure detail |

Do not treat every red CI run or non-zero exit code as "Protocol Canary
found an incompatibility" — check `status`/exit code first to know which
kind of red you're looking at.
