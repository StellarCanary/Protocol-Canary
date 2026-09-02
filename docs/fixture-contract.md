# Fixture loading contract

This document is the precise, verified contract between `Protocol-Canary`
(this repository) and any external source of fixtures, including the
planned `ProtocolCanary-Fixtures` repository. It reflects the actual
current implementation — nothing here is aspirational.

## 1. Where fixtures are loaded from

A plain local filesystem directory, given to `canary_fixtures::load_directory(path)`.
The `stellar-canary check` and `stellar-canary fixtures` CLI commands take
this path via `--fixtures-dir` (default: `fixtures`, resolved relative to
the current working directory). There is no other loading mechanism —
no network fetch, no package registry, no database.

If `--fixtures-dir` does not exist, the CLI treats that as zero fixtures
(not an error): `check` still runs (with `0/0 applicable checks`, a
trivial pass) and `fixtures` reports "no fixtures found". Calling
`canary_fixtures::load_directory` directly on a nonexistent path, however,
*does* return an error (`FixtureError::ReadDir`) — the CLI's
`is_dir()` guard is what makes a missing directory non-fatal.

## 2. Directory structure

Any directory tree. The loader recursively walks `--fixtures-dir` and
collects every file ending in `.toml`, in every subdirectory, at any
depth. Subdirectory names (e.g. `xdr/`, `rpc/`, `soroban/`, or
`protocol-28/xdr/`) are a convention for human organization only — the
loader does not read directory names to determine a fixture's surface or
protocol. Non-`.toml` files (READMEs, licenses, etc.) are silently
ignored.

Fixtures are loaded in sorted-by-path order, which is what makes
`stellar-canary fixtures` and `stellar-canary check`'s output
deterministic.

## 3. The fixture file schema

Each `.toml` file is exactly one fixture. Every fixture file has:

```toml
id = "unique-string"              # required
protocol = 28                      # required, integer
surface = "xdr"                    # required: "xdr" | "rpc" | "soroban"
category = "cap-83"                # required, free-text
description = "..."                 # required, free-text
source_reference = "CAP-0083"       # optional string
required_capabilities = ["soroban-contract"]  # optional array, see below

# optional: paths (relative to this file's own directory) to externally
# stored input/expected data, resolved and existence-checked by the
# validator but otherwise unread by canary-fixtures itself
input_file = "large-payload.xdr.b64"
expected_file = "expected.xdr.b64"

# everything else in the file — the "body" — is surface-specific and is
# NOT interpreted by canary-fixtures at all; it is handed, as raw TOML,
# to the surface crate that knows how to parse it (see section 5).
```

`required_capabilities` values are the kebab-case form of
`canary_core::Capability`: `"soroban-contract"`, `"rpc-client"`,
`"stellar-sdk-dependency"`, `"wasm-artifact"`, `"raw-ledger-access"`. A
fixture requiring a capability the target project does not have (per
`canary-project`'s detection, or an explicit `[project].type` override)
is skipped, not failed — see section 6 of `docs/protocol-28.md`'s sibling
concept, the planner's skip reasons.

Validation performed on the whole loaded set (`canary_fixtures::validate`)
before anything runs:

- every `id` is unique across the entire directory tree (not just within
  one file) — a duplicate is `FixtureError::DuplicateId`, exit code 4;
- every `input_file`/`expected_file`, if present, resolves to a file that
  actually exists — a dangling reference is
  `FixtureError::MissingReferencedFile`, exit code 4.

A structurally invalid fixture (bad TOML, missing a required field, an
unrecognized `surface` string) fails at parse time with
`FixtureError::Parse`/`Read`, also exit code 4 — never silently treated
as a project incompatibility.

## 4. Fixture ID resolution

The `id` field is the fixture's identity everywhere: it is the string
shown in `stellar-canary fixtures`, the `testId`/`fixtureId` in
`CompatibilityResult` and the JSON report, and the key duplicate-checking
validates against. IDs are plain strings with no required naming
convention, though this repository's own fixtures follow
`p<protocol>-<surface>-<slug>` (e.g. `p28-xdr-cap83-empty-tx-set`) for
readability — that convention is not enforced by the loader.

## 5. Protocol version selection

Each fixture declares its own `protocol = N`. A run's *target* protocol
comes from (in order of precedence) `--protocol`, then `.stellar-canary.toml`'s
`protocol` field, then the built-in default (28). The planner
(`canary_runner::build_plan`) compares every loaded fixture's `protocol`
against the run's target and **skips** (does not fail) any fixture whose
protocol does not match, with a reason string
(`"fixture targets protocol X, this run targets protocol Y"`).

This means a single `--fixtures-dir` can safely contain fixtures for
multiple protocol versions side by side (e.g. `protocol-27/` and
`protocol-28/` fixtures in the same tree) — running with `--protocol 28`
automatically ignores the protocol-27 ones rather than erroring.

## 6. Surface distinction (XDR / RPC / Soroban)

The fixture's `surface` field (`"xdr"`, `"rpc"`, or `"soroban"`) selects
which surface crate parses its body:

| `surface` | Parsed by | Expected body fields |
|---|---|---|
| `"xdr"` | `canary_xdr::XdrFixture::from_loaded` | `type` (currently `"StellarValue"` or `"ContractExecutable"`), `kind` (`"decode-success"` \| `"decode-failure"` \| `"roundtrip"` \| `"encode-equals"`), `value_base64`, and `expected_base64` (only for `encode-equals`) |
| `"rpc"` | `canary_rpc::RpcFixture::from_loaded` | `method` (`"get-network"` \| `"get-latest-ledger"`), `[[assert]]` array of `{kind, field, value?, expected_type?}` |
| `"soroban"` | `canary_soroban::SorobanFixture::from_loaded` | `source_account`, `contract_id`, `function`, `sequence_number`, optional `[[args]]`, and `[expect]` (`{kind = "simulation-success"}` or `{kind = "simulation-error", message_contains?}`) |

An unrecognized `surface` string fails to parse (`serde` rejects it at
the `RawFixtureFile` level, surfacing as `FixtureError::Parse`) before it
ever reaches a surface crate.

## 7. Can this loader already consume `ProtocolCanary-Fixtures` with zero code changes?

**Yes.** The loader has no awareness of Git, repositories, or where a
directory came from — it only reads a filesystem path. `ProtocolCanary-Fixtures`
(or any GitHub Action wrapping this CLI) needs to:

1. Get the fixtures onto local disk in the format above (a `git clone`/
   `actions/checkout`, or an already-vendored copy — no code in this
   repository cares which);
2. Pass that path via `stellar-canary check --fixtures-dir <path>` (or
   `--fixtures-dir <path>/protocol-28` to scope to one protocol pack,
   though this isn't required since step 5's protocol filtering already
   handles a mixed-protocol directory correctly).

No change to `Protocol-Canary` is required for this to work today. The
three fixtures under `tests/fixtures/protocol-28/` in this repository are
already an existence proof: they were authored using exactly this schema
and consumed with exactly this mechanism.

## 8. If a change were ever needed

If `ProtocolCanary-Fixtures` eventually wants something this contract
doesn't support — for example, a remote fixture bundle fetched and
verified by version rather than a pre-existing local checkout — that is
explicitly **out of scope for this MVP** per this repository's own
remote-fixture-safety rule (see `CONTRIBUTING.md`), and would be a new,
separate, deliberately-scoped feature rather than a change to this
contract.
