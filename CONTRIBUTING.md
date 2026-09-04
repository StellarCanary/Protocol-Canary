# Contributing

## Development setup

You need the Rust toolchain pinned in `rust-toolchain.toml` (1.91.0,
including `rustfmt` and `clippy`). `rustup` will install it automatically
the first time you run a `cargo` command in this repository.

```bash
cargo check --workspace --all-targets
cargo test --workspace
```

## Workspace structure

This is a Cargo workspace. Each crate has one responsibility:

| Crate | Responsibility |
|---|---|
| `canary-cli` | Command-line interface (`stellar-canary` binary) |
| `canary-core` | Domain model, compatibility test trait, planner, policy |
| `canary-config` | `.stellar-canary.toml` loading and validation |
| `canary-project` | Project type detection |
| `canary-fixtures` | Fixture schema, loading, validation |
| `canary-xdr` | XDR compatibility runner (uses `stellar-xdr`) |
| `canary-rpc` | Stellar RPC client and RPC compatibility runner |
| `canary-soroban` | Soroban transaction construction and simulation runner |
| `canary-runner` | Scheduling, execution, and result aggregation |
| `canary-report` | Terminal, JSON, and Markdown reporters |
| `canary-git` | Git repository metadata |

Dependencies flow one direction: `canary-cli` depends on everything else;
the surface-specific runner crates (`canary-xdr`, `canary-rpc`,
`canary-soroban`) depend on `canary-core` for shared types but not on each
other.

## Test commands

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

All four must pass before a change is considered done. Unit tests must not
require network access; anything that talks to a real RPC endpoint belongs
in `tests/integration` and must be explicitly opt-in.

## Coding standards

- No `unwrap()`/`expect()` in non-test code unless the invariant really
  cannot be violated, and the reason is documented at the call site.
- Use `thiserror` for library error types; reserve `anyhow` for CLI/binary
  boundaries.
- No floating-point arithmetic for protocol-sensitive values.
- Keep async to I/O boundaries (RPC calls); parsing, policy, and formatting
  stay synchronous.
- Don't add a dependency for something the standard library or an existing
  workspace dependency already solves. When you do add one, check its
  maintenance status, license, and MSRV against `rust-toolchain.toml`.

## Commit style

Use Conventional Commits, one logical change per commit:

```text
feat(core): add compatibility result model
fix(rpc): handle missing protocolVersion field
test(xdr): add roundtrip fixture coverage
docs: add protocol 28 compatibility guide
```

Never commit with `git add .`; stage the specific files that belong to the
change.

## Adding a new protocol pack

Protocol-specific assumptions live behind the `ProtocolPack` abstraction in
`canary-core`, not scattered through the runners. To add support for a new
protocol version:

1. add fixtures for the new protocol to `ProtocolCanary-Fixtures` (or the
   local fixture directory used in development/tests);
2. add a `ProtocolPack` entry describing which fixtures apply;
3. do not modify or delete the previous protocol's pack or fixtures.

## Adding a fixture

A fixture must declare: a unique ID, a protocol version, a surface (`xdr`,
`rpc`, or `soroban`), an assertion kind, and its expected outcome. Every
fixture must cite a real upstream source (a CAP number, an XDR definition,
or an official release) in its metadata — do not invent scenarios that
cannot be traced to real protocol behavior.
