# CA-02: JSON fixture inventory

Status: frozen for implementation planning, contract version 1.
Authoritative for: the output of `stellar-canary fixtures --format json`.

## 1. Existing behavior

Checked against `main` at `1ff7908`:

- `stellar-canary fixtures [--protocol N] [--fixtures-dir DIR] [--config PATH]`
  prints a text list of the fixture ids for one protocol, grouped under the
  headings `XDR`, `RPC` and `Soroban`, and `(no fixtures found in DIR)` when the
  protocol has none. It has no `--format` option.
- It loads and validates the whole directory first, so an invalid fixture is exit
  code `4` before anything is printed. A directory that does not exist loads as
  zero fixtures (no error). It never touches the network and does not look at the
  project, so capabilities and applicability are not evaluated (that is
  `inspect`, CA-03).
- The protocol is `--protocol`, else the configuration file's `protocol`, else 28.
- The text output names the directory the user typed. It prints no other path.
- Within a surface the order is the order the fixtures were loaded, which is
  sorted by path. Renaming a file can reorder the output.

## 2. Command

```text
stellar-canary fixtures [--protocol N] [--fixtures-dir DIR] [--config PATH]
                        [--format terminal|json] [--json]
```

`--format` defaults to `terminal`, which is today's output, byte for byte.
`--json` is shorthand for `--format json`, as on `check`. `markdown` is not
accepted. The JSON is written to stdout and nothing else is written to stdout.
Diagnostics go to stderr.

## 3. Output (`inventoryVersion` 1)

Illustrative, not real data:

<!-- contract-example: fixtures-json-v1 -->
```json
{
  "inventoryVersion": 1,
  "toolVersion": "0.0.0",
  "fixtureFormat": 1,
  "protocol": 28,
  "directoryFound": true,
  "counts": { "total": 2, "xdr": 1, "rpc": 1, "soroban": 0 },
  "otherProtocols": { "27": 3 },
  "fixtures": [
    {
      "id": "example-rpc-network",
      "protocol": 28,
      "surface": "rpc",
      "category": "network",
      "description": "illustrative entry",
      "path": "rpc/example-rpc-network.toml",
      "requiredCapabilities": [],
      "sourceReference": "https://example.invalid/reference",
      "method": "get-network"
    },
    {
      "id": "example-xdr-roundtrip",
      "protocol": 28,
      "surface": "xdr",
      "category": "example",
      "description": "illustrative entry",
      "path": "xdr/example-xdr-roundtrip.toml",
      "requiredCapabilities": ["stellar-sdk-dependency"],
      "type": "StellarValue",
      "kind": "roundtrip"
    }
  ]
}
```

### Fields

| Field | Type | Always present | Meaning |
|---|---|---|---|
| `inventoryVersion` | integer | yes | `1`. A breaking change to this shape bumps it. |
| `toolVersion` | string | yes | The CLI's version. |
| `fixtureFormat` | integer | yes | The fixture file format the engine reads (`1` today). |
| `protocol` | integer | yes | The protocol listed (flag, configuration, or the default 28). |
| `directoryFound` | boolean | yes | Whether `--fixtures-dir` exists as a directory. `false` is not an error. |
| `counts` | object | yes | `total`, `xdr`, `rpc`, `soroban` for the listed protocol. |
| `otherProtocols` | object | yes | Count of fixtures in the directory for every *other* protocol, keyed by the protocol number as a string, in ascending numeric order. `{}` when there are none. This is how a user sees "fixtures exist, but not for this protocol". |
| `fixtures` | array | yes | One entry per fixture of `protocol`; see ordering. |
| `fixtures[].id` | string | yes | The fixture's identity (unique across the directory). |
| `fixtures[].protocol` | integer | yes | Equal to `protocol`. |
| `fixtures[].surface` | string | yes | `xdr`, `rpc` or `soroban` (lowercase). |
| `fixtures[].category` | string | yes | From the file. |
| `fixtures[].description` | string | yes | From the file. |
| `fixtures[].path` | string | yes | Path of the fixture file relative to `--fixtures-dir`, with `/` separators on every platform. Never absolute, never contains `..`. |
| `fixtures[].requiredCapabilities` | array of string | yes | Sorted, lowercase kebab (`soroban-contract`, `rpc-client`, `stellar-sdk-dependency`, `wasm-artifact`, `raw-ledger-access`). `[]` when none. |
| `fixtures[].sourceReference` | string | when the file has one | The `source_reference` value. Omitted, not `null`, when absent. |
| `fixtures[].method` | string | `rpc` fixtures | The RPC method value (`get-network`, ...). |
| `fixtures[].type`, `.kind` | string | `xdr` fixtures | The XDR type name and the kind (`decode-success`, `roundtrip`, ...). |
| `fixtures[].function` | string | `soroban` fixtures | The contract function name. |

Reserved optional fields, absent until their contract is implemented. They are
never invented: no record means no field.

| Field | Contract | Meaning |
|---|---|---|
| `fixtures[].sha256` | CF-02 | SHA-256 of the fixture file's bytes, lowercase hex. |
| `fixtures[].payloads` | CF-02 | `[{ "path", "sha256" }]` for `input_file` and `expected_file`. |
| `fixtures[].verification` | CF-04 | The fixture's verification record, copied as stored. |
| `packDigest` | CF-02 | `sha256:` digest of the pack the directory holds. |

### Ordering and determinism

`fixtures` is sorted by `id`, comparing UTF-8 bytes, so the order does not depend
on file names, directory layout or the filesystem. Object keys appear in the order
shown. The text is UTF-8 with two-space indentation, LF line endings and a final
newline. Two runs over the same fixture bytes, configuration and flags produce
identical output on every platform. The output contains no timestamp, no host
name, no absolute path and no environment value.

## 4. Failure behavior

| Situation | Exit code | stdout | stderr |
|---|---|---|---|
| Normal, including zero fixtures or a missing directory | 0 | the document | empty |
| Invalid fixture, duplicate id, unsafe reference, symbolic link | 4 | empty | the error |
| Invalid or unreadable configuration, invalid `--protocol` | 2 | empty | the error |
| Unsupported `--format` value | 2 (argument parsing) | empty | usage error |

A failure never prints a partial or empty-looking JSON document, so a consumer
that parses stdout can treat "not parseable" as failure. The `fixtures` command
does not apply the D-02 empty-run rule: listing nothing is a valid answer and is
the way to find out why `check` would refuse.

## 5. Compatibility

- Consumers ignore unknown fields. New optional fields are additive within
  `inventoryVersion` 1; changing the meaning of a field, removing one, or adding
  a value to the closed `surface` set is breaking and bumps the version.
- `otherProtocols` keys are strings because JSON object keys are strings.
- The terminal output is unchanged, so existing scripts that parse it keep working.

## 6. Security and trust boundaries

Everything in the document except `inventoryVersion`, `toolVersion`,
`fixtureFormat`, `protocol` and the counts is read from fixture files, which are
untrusted data. `description`, `category`, `sourceReference` and `path` may contain
any text; consumers render them as text and never as HTML or Markdown without
escaping. The command reads files only inside `--fixtures-dir` under the loader
rules of `docs/fixture-contract.md` (no symbolic links, confined payload paths).
It runs no fixture and makes no network call.

## 7. Validation and test requirements

- A JSON Schema, `schemas/fixtures-inventory-v1.schema.json`, kept in the engine
  repository, and a test that every output of the command validates against it.
  Unknown properties are allowed.
- Golden-file tests over a small fixture tree containing all three surfaces,
  with the expected output committed and compared byte for byte.
- Ordering: renaming files and moving them between subdirectories does not change
  the output except for `path`.
- Paths use `/` and are relative when the tree is built on Windows (the test runs
  on all three CI platforms).
- Protocol filter: only the selected protocol appears in `fixtures`, and
  `otherProtocols` accounts for the rest; the sum equals the directory total.
- Missing directory and empty directory both give `directoryFound` as expected
  and `fixtures: []` with exit 0.
- Invalid fixtures give exit 4 with empty stdout.
- Capability lists are sorted and use the documented spellings.
- `--format terminal` output is byte-identical to the output before the change.

## 8. Cross-repository dependencies

CF-02 supplies the reserved digest fields; CF-04 the verification record. The
Fixtures repository can use the JSON in CI to assert that its README and registry
list the same ids (the engine is the reference loader). The Action does not call
this command.

## 9. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CA-02-A `--format`/`--json` option and the document for the fields in section 3 | Protocol-Canary | none |
| CA-02-B JSON Schema and the conformance test | Protocol-Canary | CA-02-A |
| CA-02-C Golden-file and cross-platform ordering tests | Protocol-Canary | CA-02-A |
| CA-02-D `sha256`, `payloads`, `packDigest` | Protocol-Canary | CF-02-F |
| CA-02-E `verification` | Protocol-Canary | CF-04-D |
| CA-02-F Documentation page in the mdBook (`cli/fixtures.md`) | Protocol-Canary | CA-02-A |
| CA-02-G Fixtures CI check that compares the engine's inventory with the registry | ProtocolCanary-Fixtures | CA-02-A, CF-02-B |
