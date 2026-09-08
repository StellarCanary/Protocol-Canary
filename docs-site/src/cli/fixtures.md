# `fixtures`

Lists the fixtures available for a given protocol, without running them.

```text
List fixtures available for a protocol

Usage: stellar-canary fixtures [OPTIONS]

Options:
      --protocol <PROTOCOL>          Protocol version to list fixtures for (default: the configured protocol, or 28 if there is no configuration file)
      --fixtures-dir <FIXTURES_DIR>  Directory containing fixture files [default: fixtures]
      --config <CONFIG>              Path to a configuration file (default: .stellar-canary.toml in the project root)
  -h, --help                         Print help
```

This loads and validates `--fixtures-dir` exactly the way `check` does
(so a malformed fixture is reported the same way, exit code `4`), but
stops before running anything — no network call, regardless of the
fixtures' surfaces.

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
