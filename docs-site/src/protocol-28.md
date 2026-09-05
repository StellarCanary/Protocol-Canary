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

## Currently implemented Protocol 28 checks

Five fixtures, verified against `https://soroban-testnet.stellar.org`:

| Fixture | Surface | What it proves |
|---|---|---|
| `p28-xdr-cap83-empty-tx-set` | XDR | A `StellarValue` using [CAP-0083](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0083.md)'s `STELLAR_VALUE_EMPTY_TX_SET` case round-trips byte-for-byte through the official `stellar-xdr` crate. |
| `p28-xdr-cap85-external-ref-roundtrip` | XDR | A well-formed [CAP-0085](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0085.md) `ContractExecutable::ExternalRef` value round-trips byte-for-byte. |
| `p28-xdr-cap85-external-ref-malformed` | XDR | A truncated encoding of the same CAP-0085 shape is correctly **rejected**, not silently accepted. |
| `p28-rpc-network` | RPC | The configured RPC endpoint's `getNetwork` response reports `protocolVersion = 28` and includes a string `passphrase` field. |
| `p28-soroban-native-asset-name` | Soroban | A real, unsigned `InvokeHostFunction` transaction can be built and simulated end-to-end, by calling the standard SEP-41 `name()` function on the network's reserved native-asset Stellar Asset Contract. |

Running this pack currently produces **5/5 PASS**. This is a factual
result about these five specific fixtures — it does not mean full
Protocol 28 compatibility.

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
  these five fixtures pass against its own configured dependencies and
  RPC endpoint.
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
