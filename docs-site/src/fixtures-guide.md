# Fixtures

A **fixture** is a single declarative compatibility assertion: one TOML
file describing one concrete thing to check — an XDR value that must
round-trip, an RPC response that must match a shape, or a Soroban call
that must simulate successfully.

## Why fixtures are separate from the engine

Fixtures live in their own repository,
[`ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures),
deliberately separate from the `Protocol-Canary` engine that runs them:

- **Fixtures are data, not code.** A fixture file is plain TOML. There is
  no field that executes a shell command, script, or arbitrary code, and
  none will be added — this is a hard trust boundary, since fixtures are
  public and consumed by CI pipelines. See
  [Security](./security.md#fixture-trust-boundary).
- **Fixtures version independently of the engine.** A new protocol pack
  can be added to `ProtocolCanary-Fixtures` without a `Protocol-Canary`
  release, as long as it fits the existing schema.
- **Anyone can supply their own fixture directory.** `--fixtures-dir`
  accepts any local path — a `ProtocolCanary-Fixtures` checkout is the
  canonical source, not the only possible one.

## Directory structure

Any directory tree works. The loader recursively walks `--fixtures-dir`
and collects every file ending in `.toml`, at any depth. Subdirectory
names (`xdr/`, `rpc/`, `soroban/`, `protocol-28/xdr/cap-0083/`) are a
convention for human navigation only — the loader does not read directory
names to determine a fixture's surface or protocol; only the fixture
file's own `surface` and `protocol` fields matter. Non-`.toml` files
(READMEs, licenses) are silently ignored. Fixtures load in
sorted-by-path order, which is what makes `stellar-canary fixtures` and
`stellar-canary check` output deterministic.

The real `protocol-28` pack looks like this:

```text
protocol-28/
├── README.md
├── rpc/
│   └── p28-rpc-network.toml
├── soroban/
│   └── p28-soroban-native-asset-name.toml
└── xdr/
    ├── cap-0083/
    │   └── p28-xdr-cap83-empty-tx-set.toml
    └── cap-0085/
        ├── p28-xdr-cap85-external-ref-malformed.toml
        └── p28-xdr-cap85-external-ref-roundtrip.toml
```

## Schema

Every fixture file has this common shape (from
[`schemas/fixture-v1.schema.json`](https://github.com/StellarCanary/ProtocolCanary-Fixtures/blob/main/schemas/fixture-v1.schema.json)
in `ProtocolCanary-Fixtures`, which mirrors the loader's actual
implementation):

```toml
id = "unique-string"                # required, pattern ^[a-z0-9][a-z0-9-]*$
protocol = 28                       # required, integer
surface = "xdr"                     # required: "xdr" | "rpc" | "soroban"
category = "cap-0083"                # required, free-text (avoid "misc"/"other")
description = "..."                  # required
source_reference = "CAP-0083"        # strongly expected: an authoritative upstream reference
required_capabilities = ["soroban-contract"]  # optional, see below

# optional: paths to externally stored input/expected data, relative to
# this file's own directory — existence-checked by the validator
input_file = "large-payload.xdr.b64"
expected_file = "expected.xdr.b64"

# everything else is surface-specific — see the per-surface tables below
```

`required_capabilities` values are kebab-case `canary_core::Capability`
names: `soroban-contract`, `rpc-client`, `stellar-sdk-dependency`,
`wasm-artifact`, `raw-ledger-access`. A fixture requiring a capability the
target project doesn't have is **skipped**, not failed.

### Per-surface body fields

| `surface` | Required body fields |
|---|---|
| `xdr` | `type` (`StellarValue` \| `ContractExecutable`), `kind` (`decode-success` \| `decode-failure` \| `roundtrip` \| `encode-equals`), `value_base64`, and `expected_base64` (only for `encode-equals`) |
| `rpc` | `method` (`get-network` \| `get-latest-ledger`), `assert` — an array of `{kind, field, value?, expected_type?}` |
| `soroban` | `source_account`, `contract_id`, `function`, `sequence_number`, optional `args`, and `expect` (`{kind = "simulation-success"}` or `{kind = "simulation-error", message_contains?}`) |

## ID resolution and protocol filtering

The `id` field is the fixture's identity everywhere — the string shown in
`stellar-canary fixtures`, the `testId`/`fixtureId` in results and the
JSON report, and what duplicate-checking validates against. IDs must be
unique across the **entire** loaded directory tree, not just one file.

Each fixture declares its own `protocol`. A run's target protocol is
compared against every loaded fixture's `protocol`; a mismatch is
**skipped** (with a stated reason), never failed. This means a single
`--fixtures-dir` can safely hold fixtures for multiple protocol versions
side by side.

## Validation

`ProtocolCanary-Fixtures` ships a structural validator:

```bash
python3 tools/validate/validate.py
```

It checks schema conformance, unique IDs, protocol/surface enums, source
references, and that every referenced file actually exists. It is
structural only — it never executes a fixture's assertion and never makes
a network call. `python3 -m unittest discover tests` runs its own test
suite.

`Protocol-Canary`'s own loader performs the load-time validation that
actually gates a `check`/`fixtures` run: a duplicate ID, a dangling
`input_file`/`expected_file` reference, or a structurally invalid file
(bad TOML, missing required field, unrecognized `surface`) fails the
whole load with [exit code `4`](./exit-codes.md) — never silently treated
as a project incompatibility.

## Testing a fixture

```bash
stellar-canary fixtures --fixtures-dir <path> --protocol <N>   # confirm it's picked up
stellar-canary check --fixtures-dir <path> --protocol <N>      # actually run it
```

## Adding a fixture or a protocol pack

See [Authoring a Fixture](./fixture-authoring.md) for the contributor
workflow, and [`ProtocolCanary-Fixtures`'s own `CONTRIBUTING.md`](https://github.com/StellarCanary/ProtocolCanary-Fixtures/blob/main/CONTRIBUTING.md)
for repository-specific review expectations. A new protocol pack follows
the same schema and directory convention as `protocol-28/` — see
`protocol-27/` in the Fixtures repository for a real example of an
intentionally not-yet-populated pack (fixtures are added only after their
upstream behavior is independently verified, never as placeholders).
