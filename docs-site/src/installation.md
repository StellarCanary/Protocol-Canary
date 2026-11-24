# Installation

Prebuilt archives and checksum manifests for the current release, Engine
`v0.3.0`, are published on GitHub [Releases](./releases.md). Alternatively,
install the current release from source using `cargo install` against the
pinned release tag.

## Prebuilt archives

| Target | Asset |
|---|---|
| Linux x86_64 | `stellar-canary-x86_64-unknown-linux-gnu.tar.gz` (the bare `stellar-canary-linux-x86_64` binary is published too) |
| macOS x86_64 | `stellar-canary-x86_64-apple-darwin.tar.gz` |
| macOS arm64 | `stellar-canary-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `stellar-canary-x86_64-pc-windows-msvc.zip` |

Each archive contains the `stellar-canary` executable (`stellar-canary.exe`
on Windows). No Linux arm64 archive is published for `v0.3.0`.

Every asset has its own `.sha256` file, and the release also publishes one
`SHA256SUMS` manifest covering all of them. Verify the download before
extracting it:

```bash
# Linux (and macOS with sha256sum installed)
curl -fsSLO https://github.com/StellarCanary/Protocol-Canary/releases/download/v0.3.0/SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
```

On macOS without `sha256sum`, use the same manifest with
`shasum -a 256 -c SHA256SUMS`. On Windows, compare the archive against its
sidecar:

```powershell
$actual = (Get-FileHash -Algorithm SHA256 stellar-canary-x86_64-pc-windows-msvc.zip).Hash
$expected = ((Get-Content stellar-canary-x86_64-pc-windows-msvc.zip.sha256) -split '\s+')[0]
$actual -eq $expected
```

The last line must print `True`; `--ignore-missing` on Unix only skips assets
you did not download, and never turns a mismatch into a pass.

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
cargo install --git https://github.com/StellarCanary/Protocol-Canary --tag v0.3.0 --locked
```

- `--tag v0.3.0` pins the exact, currently verified release — never
  `main`, and never an unpinned "latest".
- `--locked` uses the exact dependency versions in the repository's own
  committed `Cargo.lock`, rather than whatever the latest compatible
  versions happen to be at install time.

Verify the install:

```bash
stellar-canary version
```

```text
stellar-canary 0.3.0
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
