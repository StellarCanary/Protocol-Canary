# Releases

## Protocol-Canary

| | |
|---|---|
| Current version | `v0.1.1` |
| Distribution | A git tag (`v0.1.1`), installed via `cargo install --git ... --tag v0.1.1 --locked`. |
| GitHub Release | **None published.** Only tags (`v0.1.0`, `v0.1.1`) exist — there is no prebuilt binary and no checksum artifact. If this changes, this page and [Installation](./installation.md) will be updated to reflect it. |
| What changed in `0.1.1` | `canary-xdr` gained support for the `"ContractExecutable"` XDR type (previously only `"StellarValue"`), needed to test CAP-0085's `CONTRACT_EXECUTABLE_EXTERNAL_REF` case. |

## ProtocolCanary-Action

| | |
|---|---|
| Current release | `v0.1.1` (a real GitHub Release, since this is a JavaScript/Node action whose bundled `dist/index.js` is committed and released) |
| Floating tag | `v1` — currently resolves to the same commit as the `v0.1.1` release tag, per standard GitHub Actions convention for major-version tags. |
| Default `version` input | `0.1.1` (the `Protocol-Canary` release it installs and runs) |

## ProtocolCanary-Fixtures

| | |
|---|---|
| Current pack | `protocol-28/` — Active |
| Distribution | No release or tag mechanism — consumed as a git checkout (`actions/checkout` with `repository: StellarCanary/ProtocolCanary-Fixtures`, or a plain `git clone`), pointed at with `--fixtures-dir`. |
| Other packs | `protocol-27/` exists but is **not yet populated** — fixtures are added only after their upstream behavior is independently verified, never as placeholders. |

## Version compatibility

| `Protocol-Canary` | Fixture pack | Target protocol | `ProtocolCanary-Action` |
|---|---|---|---|
| `v0.1.1` | `ProtocolCanary-Fixtures` `protocol-28/` | 28 | `v1` (default `version: "0.1.1"`) |

This is the only combination that has actually been run together and
verified end-to-end (5/5 PASS against live Testnet). `v0.1.0` is
installable but predates `ContractExecutable` XDR support (needed by two
current Protocol 28 fixtures) and the `counts` field in its JSON report —
the Action tolerates the missing field, but `v0.1.1` is the version this
table verifies against.

## Tags vs. releases vs. binaries — what actually exists

| | Git tags | GitHub Releases | Prebuilt binaries |
|---|---|---|---|
| `Protocol-Canary` | Yes (`v0.1.0`, `v0.1.1`) | No | No |
| `ProtocolCanary-Action` | Yes (`v0.1.1`, `v1`) | Yes | N/A (a JS action; its "binary" is the committed `dist/index.js`) |
| `ProtocolCanary-Fixtures` | No | No | N/A (not a distributable binary) |

Do not assume a `Protocol-Canary` binary download exists anywhere — every
documented install path in [Installation](./installation.md) builds from
source.
