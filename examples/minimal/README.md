# Minimal example

The smallest valid `.stellar-canary.toml`: every other field falls back to
its documented default (`[project].type = "auto"`, all three surfaces
enabled, `warnings_are_failures = false`).

Run it from the workspace root:

```bash
cargo run -p canary-cli --manifest-path ../../Cargo.toml -- check
```

(or, from this directory, `stellar-canary check` once you've installed the
binary — see the root [README](../../README.md#quick-start)).

With no `fixtures/` directory present and no protocol-28 fixtures in
`--fixtures-dir` (which defaults to `fixtures`), this reports
`0/0 applicable checks passed` and exits `0`: an empty check is a
trivial pass, not an error. Because `[tests].rpc` and `[tests].soroban`
default to `true`, it still calls the real testnet RPC endpoint once to
report the observed network protocol — pass `--fixtures-dir` pointing at
[`tests/fixtures/protocol-28`](../../tests/fixtures/protocol-28) from the
repository root to see it evaluate real fixtures instead of an empty set.
