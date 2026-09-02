# Protocol 28 compatibility pack

This document describes exactly what `stellar-canary check` verifies for
Stellar Protocol 28 today, using the fixtures shipped in
`tests/fixtures/protocol-28/`, and what it does not (yet) verify.

## What Protocol 28 actually changed

Protocol 28 includes, among other changes, three CAPs relevant to this
project's three compatibility surfaces:

- **[CAP-0083](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0083.md)**
  gives validators a way to vote to drop a transaction set from the ledger
  being closed, by adding a new `StellarValueType` case,
  `STELLAR_VALUE_EMPTY_TX_SET`, to the `StellarValue` union. A `StellarValue`
  using this case carries the zero hash in its top-level `txSetHash` field,
  with the real previous transaction set hash moved into the nested
  `proposedValue.txSetHash`.
- **CAP-0085** introduces externally managed contract executable
  references for Soroban.
- **CAP-0086** adds sparse-map host functions to support efficient
  contract-storage migration.

## What this pack currently checks

| Fixture | Surface | What it proves |
|---|---|---|
| `p28-xdr-cap83-empty-tx-set` | XDR | A `StellarValue` using the CAP-0083 `STELLAR_VALUE_EMPTY_TX_SET` ext round-trips byte-for-byte through the project's configured `stellar-xdr` dependency. |
| `p28-rpc-get-network` | RPC | The configured RPC endpoint's `getNetwork` response reports protocol 28 and includes a string `passphrase` field — the live network-identity check described in the project's protocol-version-detection rule. |
| `p28-soroban-native-asset-name` | Soroban | A real, unsigned `InvokeHostFunction` transaction can be built and simulated end-to-end against live Protocol 28 infrastructure, by calling the standard SEP-41 `name()` function on the network's reserved native-asset Stellar Asset Contract. |

All three were run against `https://soroban-testnet.stellar.org` and passed;
see each fixture file's own header comment for the exact verification date
and the real response observed.

## What this pack does not check

- **CAP-0085 and CAP-0086 host-function semantics specifically.** The
  Soroban fixture above proves the construction → simulation → result
  pipeline works, but it does not exercise externally managed contract
  executables or sparse-map host functions — doing that correctly requires
  a test contract built against those exact upstream interfaces, which is
  expected to come from the dedicated `ProtocolCanary-Fixtures` repository
  rather than be guessed at here (see the project's rule against inventing
  fixture scenarios that cannot be traced to real protocol behavior).
- **Anything about a specific downstream application beyond the declared
  fixtures.** A passing run means the fixtures above passed against the
  project's configured dependencies and RPC endpoint; see the
  [Limitations](../README.md#limitations) section of the README.
- **Mainnet.** These fixtures were verified against testnet. Running them
  against mainnet requires `--network mainnet --rpc-url <endpoint>`
  explicitly (see the project's network-safety rule) and has not itself
  been separately verified here.

## Adding more Protocol 28 fixtures

See [CONTRIBUTING.md](../CONTRIBUTING.md#adding-a-fixture) for the fixture
file format. Every new fixture must cite a real upstream source (a CAP
number, an XDR definition, or an official RPC/host-function reference) in
its `source_reference` field and, ideally, its own header comment
describing how its expected values were derived or observed — the same
way the three fixtures here do.
