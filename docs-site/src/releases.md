# Releases

## Protocol-Canary

| | |
|---|---|
| Current version | `v0.2.0` |
| Distribution | A git tag (`v0.2.0`), installed via `cargo install --git https://github.com/StellarCanary/Protocol-Canary --tag v0.2.0 --locked`, or prebuilt Linux binary from GitHub Release. |
| GitHub Release | [`v0.2.0`](https://github.com/StellarCanary/Protocol-Canary/releases/tag/v0.2.0) is published (target commit `7e1eb63dd63be6aefcb9d248ae80afc66436c4c6`). It carries prebuilt binary `stellar-canary-linux-x86_64` and `SHA256SUMS`. Checksums are verified with `sha256sum -c SHA256SUMS`. |
| What changed in `0.2.0` | `check` fails by default when zero fixtures execute (exit code `2`, `--allow-empty` opts out), fixture directories containing symbolic links or unsafe payload paths are rejected, and results record `results[].source`. |

### Previous engine release: `0.1.1`

[`v0.1.1`](https://github.com/StellarCanary/Protocol-Canary/releases/tag/v0.1.1) is published (target commit `919668859bc1bef3d58737f13695b486d67632ea`), preserved unchanged. It carries no prebuilt binary and installs from source via `--tag v0.1.1`. `ProtocolCanary-Action` keeps its default pinned to `0.1.1`.

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
| Distribution | The `protocol-28/` pack is published as a tagged snapshot and [GitHub Release](https://github.com/StellarCanary/ProtocolCanary-Fixtures/releases/tag/protocol-28) — tag `protocol-28`, commit `75ec2c293a44f09d9c8c1f76722a24a2334ac20a` — the immutable, verified Protocol 28 fixture pack. It is not a compiled or binary artifact: consumers still use a git checkout/clone (`actions/checkout` with `repository: StellarCanary/ProtocolCanary-Fixtures`, or a plain `git clone`, optionally at the `protocol-28` tag/ref) and point `--fixtures-dir` at the resulting declarative TOML fixture corpus. |
| Other packs | `protocol-27/` exists but is **not yet populated** — fixtures are added only after their upstream behavior is independently verified, never as placeholders. |

## Version compatibility

| `Protocol-Canary` | Fixture pack | Target protocol | `ProtocolCanary-Action` |
|---|---|---|---|
| `v0.2.0` | `ProtocolCanary-Fixtures` `protocol-28/` | 28 | `v1` (explicit `version: "0.2.0"`) |
| `v0.1.1` | `ProtocolCanary-Fixtures` `protocol-28/` | 28 | `v1` (default `version: "0.1.1"`) |

`v0.2.0` is verified against both the `protocol-28` pack and `ProtocolCanary-Action` (including `--allow-empty` and empty-run refusal handling). `v0.1.0` is
installable but predates `ContractExecutable` XDR support (needed by two
current Protocol 28 fixtures) and the `counts` field in its JSON report —
the Action tolerates the missing field, but `v0.1.1` is the version this
table verifies against.

## Tags vs. releases vs. binaries — what actually exists

| | Git tags | GitHub Releases | Prebuilt binaries |
|---|---|---|---|
| `Protocol-Canary` | Yes (`v0.1.0`, `v0.1.1`, `v0.2.0`) | Yes (`v0.1.1`, `v0.2.0`) | Yes (Linux x86_64 for `v0.2.0`) |
| `ProtocolCanary-Action` | Yes (`v0.1.1`, `v1`) | Yes | N/A (a JS action; its "binary" is the committed `dist/index.js`) |
| `ProtocolCanary-Fixtures` | Yes (`protocol-28`) | Yes (`protocol-28`) | N/A (not a distributable binary) |

Prebuilt binaries for Linux x86_64 are available for `v0.2.0` on GitHub Releases. Alternative install paths build from source via Cargo as documented in [Installation](./installation.md).
