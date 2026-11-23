# Protocol 28

Protocol 28 is the only protocol version Protocol Canary currently
supports. This page describes exactly what the currently implemented
Protocol 28 checks verify, using the real fixture pack in
[`ProtocolCanary-Fixtures/protocol-28`](https://github.com/StellarCanary/ProtocolCanary-Fixtures/tree/main/protocol-28)
— and, just as importantly, what they do not.

Protocol 28 is implemented in Stellar Core 28.0.0, Stellar RPC 28.0.0, and
the official `stellar-xdr` 28.0.0 crate. Among its changes, three CAPs are
relevant to this project's three compatibility surfaces:

```text
Protocol 28
    ├── CAP-0083  (validators can vote to drop a transaction set)
    ├── CAP-0085  (externally managed contract executables / fleet upgrades)
    └── CAP-0086  (sparse-map host functions for storage migration)
```

## What the current pack contains

At fixture-pack revision [`e9d098d`](https://github.com/StellarCanary/ProtocolCanary-Fixtures/tree/e9d098df7931a9b94b183c6af807a58336b7b322/protocol-28),
the `protocol-28` pack holds **93 fixture definitions: 86 XDR, 2 RPC, and 5
Soroban**. That is the pack's inventory, not the number of checks any one
project runs: a run loads the fixtures for its target protocol, then applies
its own configuration (enabled surfaces, RPC endpoint, project capabilities,
`--fixture`/`--surface`), so a report covers only the applicable subset.

The five fixtures below are the historical first-run subset, verified
against `https://soroban-testnet.stellar.org`. They illustrate each surface;
they are not the pack:

| Fixture | Surface | What it proves |
|---|---|---|
| `p28-xdr-cap83-empty-tx-set` | XDR | A `StellarValue` using [CAP-0083](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0083.md)'s `STELLAR_VALUE_EMPTY_TX_SET` case round-trips byte-for-byte through the official `stellar-xdr` crate. |
| `p28-xdr-cap85-external-ref-roundtrip` | XDR | A well-formed [CAP-0085](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0085.md) `ContractExecutable::ExternalRef` value round-trips byte-for-byte. |
| `p28-xdr-cap85-external-ref-malformed` | XDR | A truncated encoding of the same CAP-0085 shape is correctly **rejected**, not silently accepted. |
| `p28-rpc-network` | RPC | The configured RPC endpoint's `getNetwork` response reports `protocolVersion = 28` and includes a string `passphrase` field. |
| `p28-soroban-native-asset-name` | Soroban | A real, unsigned `InvokeHostFunction` transaction can be built and simulated end-to-end, by calling the standard SEP-41 `name()` function on the network's reserved native-asset Stellar Asset Contract. |

The "**5/5 PASS**" transcript quoted in early documentation is this
five-fixture subset's result, not a statement about the current pack and
not a claim of full Protocol 28 compatibility. Fixture counts are not
checks run, and neither is a network observation: only the RPC and Soroban
fixtures contact a live network, and any claim about live behavior rests on
the dated verification recorded in the pack's evidence files.

## What this pack does not check

- **CAP-0085 host-function semantics beyond the wire format.** The two
  CAP-0085 fixtures prove the *encoding* round-trips; they do not deploy a
  real externally-managed-executable contract fleet end-to-end (owner
  contract, `ExecutableTagObject` entry, an instance referencing it, and a
  resolved invocation). Building that verifiably requires upstream `stellar`
  CLI support for constructing such a deployment, which was not yet
  available (`27.1.0`) when this pack was authored.
- **CAP-0086 sparse-map host functions — a documented gap, not an
  oversight.** CAP-0086 adds no new top-level XDR type; it is
  host-environment behavior only observable by invoking a deployed
  contract that calls `sparse_map_new_from_linear_memory` /
  `sparse_map_unpack_to_linear_memory`. As of this pack's release, the
  latest published `soroban-sdk` does not expose these functions at any
  stable, documented API surface, so no fixture exists for them yet. This
  gap closes once the SDK exposes them or a verified deployed contract
  using them can be pointed to — see [Roadmap](./roadmap.md).
- **Anything about a specific downstream application** beyond whether
  the fixtures applicable to it pass against its own configured
  dependencies and RPC endpoint.
- **Mainnet.** Every fixture in this pack was verified against Testnet
  only. Running against mainnet requires `--network mainnet --rpc-url <endpoint>`
  explicitly and has not itself been separately verified.

## Consuming this pack

```bash
stellar-canary check --fixtures-dir <checkout>/protocol-28 --protocol 28 --json
# or, scanning every protocol pack in a fixtures checkout at once:
stellar-canary check --fixtures-dir <checkout> --protocol 28 --json
```

Both forms work identically — see [Fixtures](./fixtures-guide.md) for why:
the loader filters by each fixture's own declared `protocol` field, not
by directory path.

## Adding more Protocol 28 fixtures

See [Authoring a Fixture](./fixture-authoring.md). Every new fixture must
cite a real upstream source (a CAP number, an XDR definition, or an
official RPC/host-function reference) in its `source_reference` field —
fixtures are never added merely to raise a coverage number.
