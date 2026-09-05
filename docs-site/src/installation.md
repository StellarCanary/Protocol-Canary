# Installation

Protocol Canary does not yet publish prebuilt binaries or checksums (see
[Releases](./releases.md)). The documented install path is `cargo install`
against a pinned release tag.

## Prerequisites

- A Rust/Cargo toolchain. The repository pins `1.91.0` in its own
  `rust-toolchain.toml`; `cargo install` will use whatever toolchain is
  active on your machine (rustup will fetch a matching one automatically
  if you use it).
- Network access to `crates.io` (for dependency resolution) and GitHub
  (to fetch the source).

No other runtime dependency is required. There is no Docker requirement
and no OS-specific instructions beyond "a working Rust toolchain" — this
project does not claim support for a specific operating system beyond
what the Rust toolchain itself supports.

## Install the current release

```bash
cargo install --git https://github.com/StellarCanary/Protocol-Canary --tag v0.1.1 --locked
```

- `--tag v0.1.1` pins the exact, currently verified release — never
  `main`, and never an unpinned "latest".
- `--locked` uses the exact dependency versions in the repository's own
  committed `Cargo.lock`, rather than whatever the latest compatible
  versions happen to be at install time.

Verify the install:

```bash
stellar-canary version
```

```text
stellar-canary 0.1.1
```

## Alternative: build from a local checkout

```bash
git clone https://github.com/StellarCanary/Protocol-Canary
cd Protocol-Canary
cargo install --path crates/canary-cli
```

## Alternative: run without installing (development)

From a workspace checkout, every command can be run directly through
Cargo without installing a binary at all:

```bash
cargo run -p canary-cli -- check
cargo run -p canary-cli -- inspect
cargo run -p canary-cli -- fixtures --protocol 28
```

## Next step

Continue to [Your First Check](./first-check.md).
