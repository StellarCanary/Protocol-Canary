# Releasing

How an engine release is cut and what to verify before and after. It describes
only what exists today: a git tag, a GitHub Release, one Linux x86_64 binary and
a `SHA256SUMS` file, all produced by `.github/workflows/release.yml`.

A checksum from the same release page as the binary protects against corruption
and truncated downloads. It does not prove who published the release. Release
signatures are not provided (decision D-07 defers them); do not describe the
checksum as authenticity.

## Before tagging

1. `main` is green on all three platforms (`CI`, `Docs`) and the commit to
   release is the tip of `main`.
2. The workspace version and every internal `version = "..."` requirement in the
   root `Cargo.toml` are the new version, `Cargo.lock` is updated, and
   `cargo metadata --no-deps --locked` reports one version.
3. `CHANGELOG.md` has a section for the version with upgrade notes for anything
   that changes the outcome of an existing invocation.
4. The canonical-pack CI job passes against the `protocol-28` tag and `main` of
   `ProtocolCanary-Fixtures`.
5. A maintainer has reviewed the evidence for the release and approved tagging.

## Tagging

```bash
git switch main && git pull --ff-only
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

Tags are never moved or deleted after they are pushed. A mistake is fixed by the
next version. The workflow refuses a tag that does not equal the workspace
version, then runs `cargo fmt --check`, `clippy -D warnings` and the tests with
`--locked`, builds with `--release --locked`, and uploads
`stellar-canary-linux-x86_64` and `SHA256SUMS` to the GitHub Release.

## After the workflow finishes

Verify the published artifacts from a clean directory, not from the build log:

```bash
mkdir verify && cd verify
gh release download vX.Y.Z -R StellarCanary/Protocol-Canary
sha256sum -c SHA256SUMS
chmod +x stellar-canary-linux-x86_64
./stellar-canary-linux-x86_64 version        # prints stellar-canary X.Y.Z
```

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

`release.yml` applies that remapping to the release build. What this does and does
not show: the same commit, toolchain, operating system and flags give the same
bytes regardless of the build directory. It does not show that a build on a
different operating system, CPU architecture or toolchain version matches, and the
published binary has not yet been compared with an independent rebuild because no
`0.2.0` release exists. Before claiming a release is reproducible, download its
asset, rebuild the tag with the command above on the same kind of machine, and
compare `sha256sum`; record the result with the release.

## What a release does not do

It does not move the Action. `ProtocolCanary-Action` pins an exact engine version
in its `version` input and its default changes only through its own reviewed
release. It does not add Protocol 29 support: that needs fixtures backed by
authoritative sources and real captures (see `docs/contracts/cf-04-verification-evidence.md`).
