# CA-03: Machine-readable inspection

Status: frozen for implementation planning, contract version 1.
Authoritative for: the output of `stellar-canary inspect --format json`.
Depends on: CF-07 (root selection, evidence), CA-02 (fixture fields), CA-04
(fingerprint).

## 1. Existing behavior

Checked against `main` at `1ff7908` (`run_inspect_inner` in
`crates/canary-cli/src/commands.rs`):

- `inspect [--protocol N] [--fixtures-dir DIR] [--config PATH]` prints plain text
  and has no `--format` option.
- It prints the absolute project root (`Project root: /home/...`), the resolved
  project type, four yes/no capability lines (Stellar SDK/XDR, Soroban contract,
  RPC client, WASM artifact), the configured protocol, the target protocol for
  the plan, whether each surface is enabled, and then the plan: the fixtures that
  would run (`xdr:`, `rpc:`, `soroban:` followed by the id) and the skipped
  fixtures with `id [surface]: reason`.
- It is offline: it builds no HTTP client and calls no endpoint.
- It exits 0 even when the plan is empty. It exits 2 for a configuration problem
  and 4 for an invalid fixture, with the error on stderr.
- `RawLedgerAccess` is a capability in the engine but is not printed.
- The absolute root path is the only machine-specific value; it is why the text
  is unsuitable to paste into an issue or to compare between machines.

## 2. Command

```text
stellar-canary inspect [--protocol N] [--fixtures-dir DIR] [--config PATH]
                       [--format terminal|json] [--json] [--verbose]
```

`terminal` is the default and is today's output byte for byte, including the
absolute root. `json` writes the document below to stdout and nothing else.
`--verbose` adds the `inputs` list of CA-04 (relative paths and hashes of every
inspected file); without it only counts and the digest are shown.

The command stays offline in every mode. It must not construct a network client,
resolve a host name, or read an environment variable.

## 3. Output (`inspectVersion` 1)

Illustrative, not real data:

<!-- contract-example: inspect-json-v1 -->
```json
{
  "inspectVersion": 1,
  "toolVersion": "0.0.0",
  "offline": true,
  "project": {
    "root": ".",
    "rootSelectedBy": "current-directory",
    "type": "soroban",
    "typeSource": "detected",
    "detectedType": "soroban",
    "capabilities": ["soroban-contract"]
  },
  "config": {
    "found": true,
    "path": ".stellar-canary.toml",
    "version": 1,
    "protocol": 28,
    "projectType": "auto",
    "tests": { "xdr": true, "rpc": true, "soroban": true },
    "policy": { "warningsAreFailures": false }
  },
  "target": { "protocol": 28, "source": "config" },
  "fixtures": {
    "directoryFound": true,
    "loaded": 3
  },
  "plan": {
    "wouldRun": [
      { "id": "example-rpc-network", "surface": "rpc" },
      { "id": "example-xdr-roundtrip", "surface": "xdr" }
    ],
    "skipped": [
      {
        "id": "example-old-protocol",
        "surface": "xdr",
        "code": "protocol-mismatch",
        "reason": "fixture targets protocol 27, this run targets protocol 28"
      }
    ],
    "checkWouldRefuse": false,
    "refusal": null
  },
  "fingerprint": {
    "fingerprintVersion": 1,
    "detector": 1,
    "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    "inputs": 2
  }
}
```

### Fields

| Field | Type | Always present | Meaning |
|---|---|---|---|
| `inspectVersion` | integer | yes | `1`. |
| `toolVersion` | string | yes | |
| `offline` | boolean | yes | Always `true`. A consumer can assert it. |
| `project.root` | string | yes | The selected root as a path relative to the directory the command was started in: `"."` when the current directory is used, or the `--project-root` value rendered relative and normalized with `/` once CF-07-A exists. Never absolute. |
| `project.rootSelectedBy` | string | yes | `current-directory` or `flag`. |
| `project.type` | string | yes | The effective type: `soroban`, `rpc-consumer`, `stellar-sdk`, `generic-stellar`, `unknown`. |
| `project.typeSource` | string | yes | `detected` or `config`. |
| `project.detectedType` | string | yes | What detection alone produced. Equals `type` when `typeSource` is `detected`; shown separately when configuration overrides it, so the override is visible (CF-07 section 6). |
| `project.capabilities` | array of string | yes | Sorted, lowercase kebab, including `raw-ledger-access` if present. |
| `project.detection` | object | when CF-07-B exists | The capability evidence document of CF-07 section 6 (`evidenceVersion` 1). Not duplicated into other fields. |
| `config.found` | boolean | yes | A configuration file was read. |
| `config.path` | string or null | yes | Relative to the root with `/`. `null` when no file was read. If `--config` points outside the root, `path` is `null` and `outsideRoot` is `true`; the location is not disclosed. |
| `config.version`, `.protocol`, `.projectType`, `.tests`, `.policy` | | yes | The effective values after defaults. `projectType` is `auto` or an explicit type. When no file exists these are the built-in defaults. |
| `target.protocol` | integer | yes | The protocol the plan was built for. |
| `target.source` | string | yes | `flag`, `config` or `default`. |
| `fixtures.directoryFound` | boolean | yes | |
| `fixtures.loaded` | integer | yes | Fixtures found in the directory, all protocols. |
| `plan.wouldRun` | array | yes | `{id, surface}` for every fixture `check` would execute. Sorted by surface (`xdr`, `rpc`, `soroban`) then `id` (UTF-8 bytes). |
| `plan.skipped` | array | yes | `{id, surface, code, reason}` for every fixture left out. `code` is `protocol-mismatch`, `surface-disabled` or `missing-capability`; `reason` is the same free text `check` records. Sorted by `id`. |
| `plan.checkWouldRefuse` | boolean | yes | `true` exactly when `plan.wouldRun` is empty, that is, when `check` would exit 2 under D-02 unless `--allow-empty` is given. |
| `plan.refusal` | string or null | yes | The same explanation text `check` prints, or `null`. This lets a script find out why before running. |
| `fingerprint` | object | when CA-04 exists | `fingerprintVersion`, `detector`, `digest`, `inputs` (count). With `--verbose` also `inputList` (CA-04). |

The document contains no absolute path, no user name, no host name, no
environment value and no timestamp. `fixtures.*` and `plan.*` carry only ids,
surfaces and counts; for full fixture detail use `fixtures --format json` (CA-02).

### Determinism

Keys in the order shown, UTF-8, two-space indent, LF, final newline. The same
project bytes, configuration, fixtures and flags give identical output on every
platform. No field depends on the time, the machine or the working directory's
absolute location.

## 4. Failure behavior

| Situation | Exit code | stdout | stderr |
|---|---|---|---|
| Any successful inspection, including an empty plan | 0 | the document | empty |
| Invalid or unreadable configuration, invalid `--protocol`, unreadable root | 2 | empty | the error |
| Invalid fixture, duplicate id, unsafe reference, symbolic link | 4 | empty | the error |

An empty plan is reported in the document (`checkWouldRefuse: true`), not as an
exit code, because the purpose of `inspect` is to explain the situation.

## 5. Compatibility

Consumers ignore unknown fields; optional additions are additive within
`inspectVersion` 1; removals, renames and new values in closed sets (`type`,
`typeSource`, `rootSelectedBy`, `skipped[].code`, `surface`) are breaking except
that new `skipped[].code` values may be added with a changelog entry and
consumers must treat an unknown code as "other". The terminal output is unchanged.

## 6. Security and trust boundaries

The project directory and the fixtures are untrusted. Inspection reads manifests
under the CF-07 boundaries (no symbolic links, no execution, bounded scan) and
fixtures under the loader rules. `reason`, `id` and any text copied from files may
be arbitrary; consumers escape them. The fingerprint digest is not a secret and not
anonymous (CA-04 section 6). Nothing here is sent anywhere.

## 7. Validation and test requirements

- Golden-file test over a fixture project for each project type, committed output
  compared byte for byte, run on Linux, macOS and Windows.
- A test that the output contains no absolute path: run in two different temporary
  directories and require identical output.
- Offline guarantee: run with an unreachable proxy/endpoint environment and with
  no network namespace where available; assert success and that no HTTP client is
  constructed (a compile-time dependency check or a mock that fails on any call).
- Plan correctness: the `wouldRun` and `skipped` sets equal what `check` executes
  and skips on the same inputs (a test that runs both and compares ids), including
  the three skip codes and `checkWouldRefuse` for each cause of an empty plan.
- Config override: explicit `[project].type` that differs from detection shows
  `typeSource: "config"` and the detected type.
- Failure rows of section 4.
- JSON Schema `schemas/inspect-v1.schema.json` and a conformance test.
- `--format terminal` is byte-identical to the current output.

## 8. Cross-repository dependencies

CF-07 (root, evidence), CA-02 (fixture detail lives there), CA-04 (fingerprint),
CF-01 (skip codes shared with the report). The Action may later call `inspect
--format json` to explain an empty plan; it must not reimplement the plan.

## 9. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CA-03-A `--format`/`--json` and the sections above except `detection` and `fingerprint` | Protocol-Canary | none |
| CA-03-B JSON Schema and conformance test | Protocol-Canary | CA-03-A |
| CA-03-C Golden files and the no-absolute-path test | Protocol-Canary | CA-03-A |
| CA-03-D Plan-equals-check consistency test | Protocol-Canary | CA-03-A |
| CA-03-E `project.detection` evidence | Protocol-Canary | CF-07-B |
| CA-03-F `fingerprint` and `--verbose` input list | Protocol-Canary | CA-04-A |
| CA-03-G `rootSelectedBy: flag` and relative root rendering | Protocol-Canary | CF-07-A |
| CA-03-H mdBook page for `inspect --format json` | Protocol-Canary | CA-03-A |
