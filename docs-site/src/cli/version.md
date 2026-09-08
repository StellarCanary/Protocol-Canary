# `version`

Prints the tool's own version.

```text
Print the tool version

Usage: stellar-canary version

Options:
  -h, --help  Print help
```

```bash
stellar-canary version
```

```text
stellar-canary 0.1.1
```

## Why version visibility matters here

`toolVersion` also appears in every [JSON report](../json-report.md),
independent of `schemaVersion` and `targetProtocol`. Because a fixture
pack can require a specific `stellar-xdr`/CLI capability (for example,
`ContractExecutable` XDR support was added in `0.1.1`, not `0.1.0` — see
[Releases](../releases.md)), knowing exactly which CLI version produced a
given report matters when comparing results across machines, across CI
runs, or against the [version compatibility table](../releases.md#version-compatibility).
