# Releases

## Protocol-Canary

| | |
|---|---|
| Current version | `v0.3.0` |
| Distribution | A git tag (`v0.3.0`), installed via `cargo install --git https://github.com/StellarCanary/Protocol-Canary --tag v0.3.0 --locked`, or a prebuilt archive from the GitHub Release. |
| GitHub Release | [`v0.3.0`](https://github.com/StellarCanary/Protocol-Canary/releases/tag/v0.3.0) is published (target commit `464c45d1bb100a009bb9dab9847c2c89ad033ab2`). It carries archives for Linux x86_64 (`stellar-canary-x86_64-unknown-linux-gnu.tar.gz`), macOS x86_64 (`stellar-canary-x86_64-apple-darwin.tar.gz`), macOS arm64 (`stellar-canary-aarch64-apple-darwin.tar.gz`) and Windows x86_64 (`stellar-canary-x86_64-pc-windows-msvc.zip`), the bare `stellar-canary-linux-x86_64` binary, a `.sha256` sidecar for every asset, and one `SHA256SUMS` manifest. |
| What changed in `0.3.0` | RPC assertions are evaluated against the endpoint's raw JSON `result`, and `get-version-info` plus further read-only RPC methods are supported; XDR fixtures can compare a decoded value to expected JSON (`decode-json-equals`); `check --fixture/--surface` runs a targeted subset; `inspect --format json` publishes the versioned inspection document; CF-04 verification evidence renders as terminal badges; `--project-root` resolves config and fixture paths; `--output` writes atomically; JSON reports gain `run.toolBuildCommit` and `skipped[].code`. |

### Installing `v0.3.0`

The Unix archives contain a `stellar-canary` executable; the Windows archive
contains `stellar-canary.exe`. Download the asset for your target, then verify
it against `SHA256SUMS` before extracting:

```bash
curl -fsSLO https://github.com/StellarCanary/Protocol-Canary/releases/download/v0.3.0/stellar-canary-x86_64-unknown-linux-gnu.tar.gz
curl -fsSLO https://github.com/StellarCanary/Protocol-Canary/releases/download/v0.3.0/SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
tar -xzf stellar-canary-x86_64-unknown-linux-gnu.tar.gz
```

On macOS, `shasum -a 256 -c SHA256SUMS` verifies the same manifest. On
Windows, `Get-FileHash -Algorithm SHA256` on the `.zip` must match its
`.sha256` sidecar. See [Installation](./installation.md) for the full asset
list. The release does not include a Linux arm64 archive.

### Previous engine release: `v0.2.0`

[`v0.2.0`](https://github.com/StellarCanary/Protocol-Canary/releases/tag/v0.2.0) is published (target commit `7e1eb63dd63be6aefcb9d248ae80afc66436c4c6`), preserved unchanged. It carries the Linux x86_64 prebuilt binary `stellar-canary-linux-x86_64` and `SHA256SUMS`, and no macOS or Windows assets; it installs from source via `--tag v0.2.0`. `check` failed by default when zero fixtures executed (exit code `2`, `--allow-empty` opts out), fixture directories containing symbolic links or unsafe payload paths were rejected, and results recorded `results[].source`.

### Earlier engine release: `v0.1.1`

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
| `v0.3.0` | `ProtocolCanary-Fixtures` `protocol-28/` | 28 | not yet verified with a published Action release (default input remains `0.1.1`) |
| `v0.2.0` | `ProtocolCanary-Fixtures` `protocol-28/` | 28 | `v1` (explicit `version: "0.2.0"`) |
| `v0.1.1` | `ProtocolCanary-Fixtures` `protocol-28/` | 28 | `v1` (default `version: "0.1.1"`) |

`v0.3.0` is the current Engine release. No `ProtocolCanary-Action` release pins
it yet — the Action's default `version` input is still `0.1.1` — so this table
does not claim that combination. `v0.2.0` is verified against both the
`protocol-28` pack and `ProtocolCanary-Action` (including `--allow-empty` and
empty-run refusal handling). `v0.1.0` is
installable but predates `ContractExecutable` XDR support (needed by two
current Protocol 28 fixtures) and the `counts` field in its JSON report —
the Action tolerates the missing field, but `v0.1.1` is the version this
table verifies against.

## Tags vs. releases vs. binaries — what actually exists

| | Git tags | GitHub Releases | Prebuilt binaries |
|---|---|---|---|
| `Protocol-Canary` | Yes (`v0.1.0`, `v0.1.1`, `v0.2.0`, `v0.3.0`) | Yes (`v0.1.1`, `v0.2.0`, `v0.3.0`) | Yes (Linux x86_64, macOS x86_64, macOS arm64, and Windows x86_64 for `v0.3.0`; Linux x86_64 only for `v0.2.0`) |
| `ProtocolCanary-Action` | Yes (`v0.1.1`, `v1`) | Yes | N/A (a JS action; its "binary" is the committed `dist/index.js`) |
| `ProtocolCanary-Fixtures` | Yes (`protocol-28`) | Yes (`protocol-28`) | N/A (not a distributable binary) |

Prebuilt archives for all four targets are available for `v0.3.0` on GitHub
Releases. Alternative install paths build from source via Cargo as documented
in [Installation](./installation.md).
