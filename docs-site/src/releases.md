# Releases

## Protocol-Canary

| | |
|---|---|
| Current version | `v0.1.1` |
| Distribution | A git tag (`v0.1.1`), installed via `cargo install --git ... --tag v0.1.1 --locked`. |
| GitHub Release | [`v0.1.1`](https://github.com/StellarCanary/Protocol-Canary/releases/tag/v0.1.1) is published (target commit `919668859bc1bef3d58737f13695b486d67632ea`). It carries no prebuilt binary and no checksum artifact — installation still builds from source via the tag, exactly as before. If prebuilt binaries are ever added, this page and [Installation](./installation.md) will be updated to reflect it. |
| What changed in `0.1.1` | `canary-xdr` gained support for the `"ContractExecutable"` XDR type (previously only `"StellarValue"`), needed to test CAP-0085's `CONTRACT_EXECUTABLE_EXTERNAL_REF` case. |

### Next engine release: `0.2.0` (prepared, not published)

The workspace and `CHANGELOG.md` are prepared for `0.2.0`, which changes the
outcome of two existing invocations: a `check` that executes nothing now fails
(exit `2`, `--allow-empty` opts out) and fixture directories containing links
or unsafe payload paths are rejected. The "Current version" above stays `v0.1.1`
until the tag exists. See `CHANGELOG.md` for the upgrade notes and
`docs/releasing.md` in the repository for the procedure and the verification a
release needs. `ProtocolCanary-Action` keeps its pinned default engine version;
it does not follow this release automatically.

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
| `Protocol-Canary` | Yes (`v0.1.0`, `v0.1.1`) | Yes (`v0.1.1`) | No |
| `ProtocolCanary-Action` | Yes (`v0.1.1`, `v1`) | Yes | N/A (a JS action; its "binary" is the committed `dist/index.js`) |
| `ProtocolCanary-Fixtures` | Yes (`protocol-28`) | Yes (`protocol-28`) | N/A (not a distributable binary) |

Do not assume a `Protocol-Canary` binary download exists anywhere — every
documented install path in [Installation](./installation.md) builds from
source.
