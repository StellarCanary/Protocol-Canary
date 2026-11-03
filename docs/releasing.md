# Releasing

How an engine release is cut and what to verify before and after. It describes
only what exists today: a versioned git tag and the GitHub Release automatically
published by `.github/workflows/release.yml`. The workflow produces release
archives for Linux x86_64, macOS x86_64 and ARM64, and Windows x86_64. It also
produces a standalone Linux x86_64 binary, individual `.sha256` checksum
files, and a combined `SHA256SUMS` manifest.

A checksum from the same release page as the binary protects against corruption
and truncated downloads. It does not prove who published the release. Release
signatures are not provided (decision D-07 defers them); do not describe the
checksum as authenticity.

## Before tagging

1. The tip of `main` is green in CI on Linux, macOS, and Windows, and the
   documentation-example and release E2E workflows have passed on that commit.
2. The workspace version and every internal `version = "..."` requirement in the
   root `Cargo.toml` are the new version, `Cargo.lock` is updated, and
   `cargo metadata --no-deps --locked` reports one version.
3. `CHANGELOG.md` has a section for the version with upgrade notes for anything
   that changes the outcome of an existing invocation.
4. The canonical-pack CI job passes against the `protocol-28` tag and `main` of
   `ProtocolCanary-Fixtures`.
5. A maintainer has reviewed the evidence for the release and approved tagging.
6. Protected release-tag rules are active for `v*.*.*`: authorized maintainer
   creation only, with updates and deletions blocked. Confirm those settings
   before pushing a tag, since a tag push automatically starts publication.

## Tagging

```bash
git switch main
git fetch origin
git pull --ff-only origin main
git rev-parse HEAD  # Verify this is the approved release commit.
git tag -a vX.Y.Z "$(git rev-parse HEAD)" -m "vX.Y.Z"
git push origin refs/tags/vX.Y.Z
```

Tags are never moved or deleted after they are pushed. A mistake is fixed by the
next version. **Pushing a `v*.*.*` tag starts the release workflow and can
publish the GitHub Release automatically; there is no separate manual
publication approval step in this workflow.**

The release workflow checks that the tagged commit is on `main` and that the
tag matches the version of every workspace crate. It runs formatting, Clippy
with warnings denied, and locked workspace tests. It then builds and packages
these targets with `cargo build --workspace --release --locked`:

| Target | Release archive |
|---|---|
| Linux x86_64 | `stellar-canary-x86_64-unknown-linux-gnu.tar.gz` |
| macOS x86_64 | `stellar-canary-x86_64-apple-darwin.tar.gz` |
| macOS ARM64 | `stellar-canary-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `stellar-canary-x86_64-pc-windows-msvc.zip` |

It also publishes the standalone Linux binary `stellar-canary-linux-x86_64`,
per-asset `.sha256` files, and the combined `SHA256SUMS` file. The publish job
verifies these checksums before uploading release assets. The release workflow
does not currently produce release signatures or provenance attestations.

## After the workflow finishes

Confirm the release workflow passed and the published release contains all
expected archives, the standalone Linux binary, individual checksums, and
`SHA256SUMS`. Verify the actual published bytes from a clean directory, not
only the workflow logs. On Linux, for example:

```bash
mkdir -p verify && cd verify
gh release download vX.Y.Z -R StellarCanary/Protocol-Canary
sha256sum -c SHA256SUMS
chmod +x stellar-canary-linux-x86_64
./stellar-canary-linux-x86_64 --version  # Reports stellar-canary X.Y.Z.
```

The `Release smoke test` workflow runs on `release.published` and downloads
the Linux x86_64, macOS ARM64, and Windows x86_64 archives. It verifies each
archive against `SHA256SUMS`, extracts it, checks the reported version, and
runs `--help`. Confirm each smoke-test job passed. The macOS x86_64 archive
is built and checksummed, but is not part of that three-target smoke matrix.
A checksum protects against a corrupted download, not a malicious publisher.

Then confirm the tag still points at the intended commit:

```bash
git ls-remote --tags origin vX.Y.Z vX.Y.Z^{}
```

Only after those checks, open a documentation pull request that changes the
install instructions and the version tables (`README.md`, `docs-site/src/index.md`,
`docs-site/src/releases.md`, `docs-site/src/cli/version.md`) to the new version.
Until the tag exists, those pages must keep saying the previous version is the
current one.

## Reproducibility

Measured on 2026-10-09 on one Linux x86_64 machine with the pinned toolchain
(`rust-toolchain.toml`, 1.91.0), building the same commit with
`cargo build --workspace --release --locked`:

| Conditions | Result |
|---|---|
| Same directory, built twice | byte-identical |
| Two different directories | **different binaries**: source paths are embedded |
| Two different directories, with the path remapping below | byte-identical |

```bash
RUSTFLAGS="--remap-path-prefix=$PWD=/build \
  --remap-path-prefix=$HOME/.cargo=/cargo \
  --remap-path-prefix=$HOME/.rustup=/rustup" \
  cargo build --workspace --release --locked
```

`release.yml` applies the above remapping to its Linux x86_64 release build,
and remaps the workspace path for macOS and Windows targets. This historical
experiment does not prove that binaries across different operating systems,
CPU architectures, or toolchains have identical bytes.
For the `v0.2.0` release, the published Linux x86_64 release asset was downloaded
and independently rebuilt from tag `v0.2.0` with the remapping above: both produced
the exact identical SHA-256 hash `34656189e15169a9f9d339288f7a8a95bb81d86ecee14ca278a165d041d28943`,
confirming byte-level independent reproducibility on Linux x86_64 with toolchain 1.91.0.

## What a release does not do

It does not move the Action. `ProtocolCanary-Action` pins an exact engine version
in its `version` input and its default changes only through its own reviewed
release. Engine `v0.3.0` includes support for the Protocol 29 fixture method
`get-version-info`, but this alone does not establish general Protocol 29
compatibility. Such claims require specific fixtures backed by authoritative
sources and real captures (see `docs/contracts/cf-04-verification-evidence.md`).
