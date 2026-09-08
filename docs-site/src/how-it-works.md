# How It Works

A single `stellar-canary check` run goes through the same pipeline whether
it runs on a developer's machine or inside a GitHub Actions job:

1. **Configuration** — `.stellar-canary.toml` (or CLI flags, which take
   precedence) determines the target protocol, network, RPC endpoint, and
   which surfaces (`xdr`/`rpc`/`soroban`) are enabled.
2. **Project detection** — the CLI inspects the current directory and
   classifies it as `soroban`, `rpc-consumer`, `stellar-sdk`,
   `generic-stellar`, or `unknown`. This determines which fixtures'
   `required_capabilities` are satisfied.
3. **Fixture loading** — every `.toml` file under `--fixtures-dir` is
   loaded recursively and validated as a whole set: unique IDs, resolvable
   file references, schema conformance. A structurally invalid fixture
   fails the whole load (exit code `4`) — it is never silently skipped.
4. **Planning** — fixtures are filtered to the ones whose declared
   `protocol` matches the run's target and whose `required_capabilities`
   the detected project satisfies. Everything else is recorded as
   **skipped**, with a stated reason, never silently dropped and never
   counted as failed.
5. **Execution** — each planned fixture runs through the surface crate
   that understands its `surface` field:
   - `xdr` — decodes/encodes against the official `stellar-xdr` crate.
     Offline, no network call.
   - `rpc` — makes a live call (`getNetwork`, `getLatestLedger`) against
     the configured RPC endpoint and checks the declared assertions.
   - `soroban` — builds an **unsigned** transaction and calls
     `simulateTransaction`. It never signs, never requires a private key,
     and never submits a transaction to any network.
6. **Policy evaluation** — the set of per-fixture results is reduced to
   one overall `status`: `pass`, `warning`, `fail`, or `error`. `error`
   (an execution problem, such as an unreachable RPC endpoint) takes
   precedence over what the pass/fail counts alone would suggest, and maps
   to its own [exit code](./exit-codes.md).
7. **Reporting** — the same result set renders as terminal output
   (default), `--format json`, or `--format markdown`.

```text
Developer / CI
     │
     ▼
.stellar-canary.toml + CLI flags
     │
     ▼
project detection ──► fixture loading & validation
     │                        │
     │                        ▼
     │                  planner (protocol + capability match)
     │                        │
     │           ┌────────────┼────────────┐
     │           ▼            ▼             ▼
     │          XDR          RPC         Soroban
     │        (offline)  (live, read)  (live, simulate-only)
     │           └────────────┼────────────┘
     │                        ▼
     │                policy evaluation (status + exit code)
     │                        │
     ▼                        ▼
terminal / JSON / markdown report
```

Only the XDR surface is offline. A run with `rpc` or `soroban` enabled in
`.stellar-canary.toml` (the default, and the case for the `protocol-28`
pack) genuinely requires network access to the configured `--rpc-url` — a
failure to reach it is reported as an execution error
([exit code `3`](./exit-codes.md)), not silently skipped. See
[Architecture](./architecture.md) for the full component breakdown and
[Troubleshooting](./troubleshooting.md#network-troubleshooting) for how to
tell an RPC outage apart from a real compatibility failure.
