# `inspect`

Prints offline project diagnostics. No fixtures run, and no network call
is made.

```text
Print offline project diagnostics

Usage: stellar-canary inspect [OPTIONS]

Options:
      --protocol <PROTOCOL>          Target protocol version (overrides configuration)
      --fixtures-dir <FIXTURES_DIR>  Directory containing fixture files to inspect [default: fixtures]
      --project-root <PATH>          Directory to treat as the project root (default: the current directory)
      --config <CONFIG>              Path to a configuration file (default: .stellar-canary.toml in the project root)
      --format <FORMAT>              Output format [default: terminal] [possible values: terminal, json]
      --json                         Shorthand for --format json
      --verbose                      Include verbose diagnostic details in output
  -h, --help                         Print help
```

## What it inspects

The same project-detection logic `check` uses internally, surfaced on its
own: the project root, its detected type, which Stellar-related
dependencies/artifacts were detected, the configured target protocol, and
which surfaces (`xdr`/`rpc`/`soroban`) are currently enabled.

With `--fixtures-dir` and `--protocol`, `inspect` drives the same
compatibility-planner preview described in the README: it lists fixtures that
would run and fixtures skipped for a protocol mismatch, disabled surface, or
missing project capability, without contacting Stellar RPC or Soroban endpoints.

## Example

Run inside a checkout of `Protocol-Canary` itself:

```bash
stellar-canary inspect
```

```text
Project root: /home/hollujay/Protocol-Canary
Project type: unknown

Detected Stellar SDK/XDR dependency: false
Detected Soroban contract usage: false
Detected RPC client dependency: false
Detected WASM artifact: false

Configured protocol: 28
Available compatibility surfaces:
  xdr:     enabled
  rpc:     enabled
  soroban: enabled
```

`Project type: unknown` here is correct — this repository is Canary's own
source code, not a Stellar application, so none of the detection
heuristics (Soroban contract, RPC client, stellar-sdk dependency, WASM
artifact) match. Run `inspect` inside your own project to see its actual
detected type and which fixtures' `required_capabilities` it will satisfy.

`inspect` is useful before `check` when you want to confirm project
detection or configuration without waiting on a live network call.

## JSON output (`--format json`)

`--format json` (or the shorthand `--json`) prints the CA-03 inspection
document: one JSON document on stdout, nothing on stderr. It still makes
no network call — `offline` is always `true` in the document. The output
is deterministic: no timestamps, and `project.root` is relative to the
current directory, never absolute. The full contract is
[CA-03](https://github.com/StellarCanary/Protocol-Canary/blob/main/docs/contracts/ca-03-inspect-json.md),
and the machine-readable schema is
[`inspect-v1.schema.json`](https://github.com/StellarCanary/Protocol-Canary/blob/main/docs/schemas/inspect-v1.schema.json).

```bash
stellar-canary inspect --fixtures-dir ProtocolCanary-Fixtures/protocol-28 --protocol 28 --json
```

```text
{
  "inspectVersion": 1,
  "toolVersion": "0.2.0",
  "offline": true,
  "project": {
    "root": ".",
    "rootSelectedBy": "current-directory",
    "type": "soroban",
    "typeSource": "detected",
    "detectedType": "soroban",
    "capabilities": [
      "soroban-contract",
      "wasm-artifact"
    ],
    "workspace": {
      "members": []
    },
    "detection": {
      "evidenceVersion": 1,
      "root": ".",
      "capabilities": [],
      "inspected": [
        "Cargo.toml"
      ],
      "diagnostics": [],
      "projectType": {
        "value": "soroban",
        "source": "detected"
      }
    }
  },
  "config": {
    "found": false,
    "path": null,
    "version": 1,
    "protocol": 28,
    "projectType": "auto",
    "tests": {
      "xdr": true,
      "rpc": false,
      "soroban": false
    },
    "policy": {
      "warningsAreFailures": false
    }
  },
  "target": {
    "protocol": 28,
    "source": "flag"
  },
  "fixtures": {
    "directoryFound": true,
    "loaded": 2
  },
  "plan": {
    "wouldRun": [
      {
        "id": "p28-xdr-cap83-empty-tx-set",
        "surface": "xdr"
      }
    ],
    "skipped": [
      {
        "id": "p27-xdr-old",
        "surface": "xdr",
        "code": "UNSUPPORTED_PROTOCOL",
        "reason": "fixture targets protocol 27, this run targets protocol 28"
      }
    ],
    "checkWouldRefuse": false,
    "refusal": null
  },
  "workspace": {
    "members": []
  },
  "fingerprint": {
    "fingerprintVersion": 1,
    "detector": 1,
    "digest": "sha256:859e9ac27cdbe3ee8bbaac66355d2e4ab98879e7c27f5d8a099e5a34ec0144bc",
    "inputs": 1
  }
}
```

(Example output trimmed; `project.detection` carries the evidence the
detector collected, and `workspace.members` lists workspace members
when one is detected.)

### Field reference

| Field | Type | Meaning |
|---|---|---|
| `inspectVersion` | integer | Document format version; always `1` today. Consumers must reject an unknown version. |
| `toolVersion` | string | The CLI's own semver. Informational — never use it for feature detection. |
| `offline` | boolean | Always `true`: `inspect` is offline in every mode. |
| `project` | object | Project root (`root`, relative), how it was selected (`rootSelectedBy`: `current-directory` or `flag`), the effective `type` and where it came from (`typeSource`: `detected` or `config`), what detection alone produced (`detectedType`), the sorted `capabilities` list, and the `detection` evidence block. |
| `config` | object | Configuration file status and effective settings: `found`, `path` (`null` when absent), `version`, `protocol`, `projectType`, the per-surface `tests` switches, and `policy.warningsAreFailures`. |
| `target` | object | The protocol actually targeted and its `source`: `flag`, `config` or `default`. |
| `fixtures` | object | `directoryFound` and `loaded`: how many fixtures the directory yielded in total (all protocols), before planning. |
| `plan` | object | The compatibility plan — see below. |
| `workspace` | object | Workspace member diagnostics. |
| `fingerprint` | object | The CA-04 project fingerprint: `fingerprintVersion`, `detector`, `digest` (`sha256:<64 lowercase hex>`) and `inputs`. The digest is not a secret and not anonymous; see CA-04. |

`project.type` and `project.detectedType` are one of `soroban`,
`rpc-consumer`, `stellar-sdk`, `generic-stellar` or `unknown`. Unknown
values in a newer document must be treated as `unknown`.

### The plan

`plan` mirrors what `check` would do with the same project, fixtures
directory and protocol, without running anything:

- `wouldRun` is the ordered list of fixtures `check` would execute, in
  execution order: XDR first, then RPC, then Soroban, each in planned
  order. A consumer can compare it against a report's `results`.
- `skipped` lists every fixture left out, sorted by `id`, each with a
  machine-readable `code`, the `surface`, and the same free-text
  `reason` `check` records. Codes are the CF-01 skip codes:
  `UNSUPPORTED_PROTOCOL`, `SURFACE_DISABLED`, `MISSING_CAPABILITY`,
  and the `UNKNOWN` fallback. New codes may be added by a changelog
  entry; consumers must treat a code they do not know as "other".
- `checkWouldRefuse` is `true` when no fixture applies — the situation
  where `check` (without `--allow-empty`) would fail with exit code `2`.
  `refusal` then holds the human-readable explanation `check` would
  print; otherwise it is `null`.

An empty plan is **reported in the document, not as an exit code**:
`inspect` still exits `0`, because its purpose is to explain the
situation.

### Examples with `jq`

```bash
# How many fixtures check would run.
stellar-canary inspect --json | jq '.plan.wouldRun | length'

# Ids and reasons for everything that gets skipped.
stellar-canary inspect --json | jq -r '.plan.skipped[] | "\(.id): \(.code) (\(.reason))"'

# Fail a CI step when check would refuse to run anything.
stellar-canary inspect --json | jq -e '.plan.checkWouldRefuse | not' > /dev/null

# The project fingerprint, e.g. to key a CI cache on.
stellar-canary inspect --json | jq -r '.fingerprint.digest'
```

### Failure behavior

| Situation | Exit code | stdout | stderr |
|---|---|---|---|
| Any successful inspection, including an empty plan | 0 | the document | empty |
| Invalid or unreadable configuration, invalid `--protocol`, unreadable root | 2 | empty | the error |
| Invalid fixture, duplicate id, unsafe reference, symbolic link | 4 | empty | the error |

A failure never prints a partial or empty-looking JSON document, so a
consumer that parses stdout can treat "not parseable" as failure. See
[Exit Codes](../exit-codes.md) for the full contract, and
[`fixtures --format json`](fixtures.md) for the fixture inventory.
