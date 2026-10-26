# `fixtures`

Lists the fixtures available for a given protocol, without running them.

```text
List fixtures available for a protocol

Usage: stellar-canary fixtures [OPTIONS]

Options:
      --protocol <PROTOCOL>          Protocol version to list fixtures for (default: the configured protocol, or 28 if there is no configuration file)
      --fixtures-dir <FIXTURES_DIR>  Directory containing fixture files [default: fixtures]
      --project-root <PATH>          Directory to treat as the project root (default: the current directory)
      --config <CONFIG>              Path to a configuration file (default: .stellar-canary.toml in the project root)
      --format <FORMAT>              Output format [default: terminal] [possible values: terminal, json]
      --json                         Shorthand for --format json
      --surface <SURFACE>            Filter fixtures by compatibility surface [possible values: xdr, rpc, soroban]
      --category <CATEGORY>          Filter fixtures by category
  -h, --help                         Print help
```

This loads and validates `--fixtures-dir` exactly the way `check` does
(so a malformed fixture is reported the same way, exit code `4`), but
stops before running anything — no network call, regardless of the
fixtures' surfaces.

As with `check`, a `--fixtures-dir` that does not exist is treated as zero
fixtures, not an error: the command exits `0` and prints the same message
as a real but empty directory, so check the path if you expected fixtures.

```text
$ stellar-canary fixtures --fixtures-dir /does/not/exist --protocol 28
Protocol 28 fixtures

(no fixtures found in /does/not/exist)
```

## Example

```bash
stellar-canary fixtures --fixtures-dir ProtocolCanary-Fixtures/protocol-28 --protocol 28
```

```text
Protocol 28 fixtures

XDR
  p28-xdr-cap83-empty-tx-set
  p28-xdr-cap85-external-ref-malformed
  p28-xdr-cap85-external-ref-roundtrip

RPC
  p28-rpc-network

Soroban
  p28-soroban-native-asset-name
```

Fixtures are grouped by surface and listed in sorted-path order, matching
`check`'s deterministic ordering. Use this to confirm which fixtures a
`--fixtures-dir`/`--protocol` combination will actually run before
spending a live network call on `check` — see [Fixtures](../fixtures-guide.md)
for what a fixture is and how this list maps to the file layout.

## JSON output (`--format json`)

`--format json` (or the shorthand `--json`) prints the CA-02 fixture
inventory: one JSON document on stdout, nothing on stderr, and no
terminal formatting. The document is deterministic — it contains no
timestamps, absolute paths or machine-dependent values — so it is safe
to diff between runs or key CI steps off of it. The full contract is
[CA-02](https://github.com/StellarCanary/Protocol-Canary/blob/main/docs/contracts/ca-02-fixtures-json.md),
and the machine-readable schema is
[`fixtures-v1.schema.json`](https://github.com/StellarCanary/Protocol-Canary/blob/main/docs/schemas/fixtures-v1.schema.json).

```bash
stellar-canary fixtures --fixtures-dir ProtocolCanary-Fixtures/protocol-28 --protocol 28 --format json
```

```text
{
  "inventoryVersion": 1,
  "toolVersion": "0.2.0",
  "fixtureFormat": 1,
  "protocol": 28,
  "directoryFound": true,
  "counts": {
    "total": 3,
    "xdr": 1,
    "rpc": 1,
    "soroban": 1
  },
  "otherProtocols": {
    "27": 1
  },
  "fixtures": [
    {
      "id": "p28-rpc-network",
      "protocol": 28,
      "surface": "rpc",
      "category": "network",
      "description": "getNetwork basics",
      "path": "rpc/network.toml",
      "requiredCapabilities": [
        "rpc-client"
      ],
      "method": "getNetwork"
    },
    {
      "id": "p28-soroban-native-asset-name",
      "protocol": 28,
      "surface": "soroban",
      "category": "token",
      "description": "native asset contract id",
      "path": "soroban/native-asset-name.toml",
      "requiredCapabilities": [
        "soroban-contract",
        "wasm-artifact"
      ],
      "function": "asset"
    },
    {
      "id": "p28-xdr-cap83-empty-tx-set",
      "protocol": 28,
      "surface": "xdr",
      "category": "ledger",
      "description": "empty transaction set roundtrip",
      "path": "xdr/empty-tx-set.toml",
      "requiredCapabilities": [
        "stellar-sdk-dependency"
      ],
      "type": "TransactionSet",
      "kind": "roundtrip"
    }
  ]
}
```

(Example output trimmed to one fixture per surface for clarity; a real
directory lists every fixture of the protocol.)

### Field reference

| Field | Type | Meaning |
|---|---|---|
| `inventoryVersion` | integer | Inventory format version; always `1` today. Consumers must reject an unknown version. |
| `toolVersion` | string | The CLI's own semver. Informational — never use it for feature detection. |
| `fixtureFormat` | integer | The fixture file format the engine reads; `1` today. |
| `protocol` | integer | The protocol version this inventory lists fixtures for. |
| `directoryFound` | boolean | Whether the fixture directory exists. `false` is not an error: it reports an empty inventory, exactly like the terminal listing. |
| `counts` | object | `total`, `xdr`, `rpc`, `soroban`: fixture counts for `protocol`. |
| `otherProtocols` | object | Count of fixtures in the directory for every other protocol, keyed by the protocol number as a string. |
| `fixtures` | array | One entry per fixture of `protocol`, sorted by `id` (comparing UTF-8 bytes). |

Each `fixtures[]` entry always carries `id`, `protocol`, `surface`,
`category`, `description`, `path` (relative to the fixture directory,
with forward slashes on every platform) and `requiredCapabilities`
(sorted, lowercase kebab). It additionally carries the surface-specific
`method` (`rpc`), `type` and `kind` (`xdr`), or `function`
(`soroban`), plus the optional `sourceReference` and `verification`
(the CF-04 verification record, copied as stored). Optional fields are
**omitted, not `null`**, when the fixture file has no value for them.
`sha256`, `payloads` and `packDigest` are reserved for CF-02 and are
not emitted yet.

### Filtering changes the counts

`--surface` and `--category` filter the inventory before the document
is built, so `counts`, `otherProtocols` and `fixtures` all describe the
filtered listing, not the whole directory:

```bash
stellar-canary fixtures --format json --surface rpc | jq '.counts'
```

### Examples with `jq`

```bash
# Every fixture id for the target protocol.
stellar-canary fixtures --format json | jq -r '.fixtures[].id'

# Ids of the RPC fixtures only.
stellar-canary fixtures --format json | jq -r '.fixtures[] | select(.surface == "rpc") | .id'

# Fail CI when the pack is empty.
stellar-canary fixtures --format json | jq -e '.counts.total > 0' > /dev/null

# Which other protocols are present in the directory.
stellar-canary fixtures --format json | jq '.otherProtocols'
```

### Failure behavior

| Situation | Exit code | stdout | stderr |
|---|---|---|---|
| Normal run, including zero fixtures or a missing directory | 0 | the document | empty |
| Invalid or unreadable configuration, invalid `--protocol`, unsupported `--format` value | 2 | empty | the error or usage message |
| Invalid fixture, duplicate id, unsafe reference, symbolic link | 4 | empty | the error |

A failure never prints a partial or empty-looking JSON document, so a
consumer that parses stdout can treat "not parseable" as failure. See
[Exit Codes](../exit-codes.md) for the full contract.
