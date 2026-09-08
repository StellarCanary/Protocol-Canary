# Roadmap

This reflects [`Protocol-Canary`'s own `ROADMAP.md`](https://github.com/StellarCanary/Protocol-Canary/blob/main/ROADMAP.md).
It is practical, scoped work that fits the current architecture — **not a
schedule with committed dates**. Items move up when someone (maintainer
or contributor) picks them up.

## Near term

- **Wire `CacheStore` into `check`.** The local, file-backed result cache
  in `canary-core` is implemented and unit-tested but not yet used by
  `check`'s execution path — every run currently calls RPC/Soroban fresh.
  Useful once fixture counts or CI frequency make rate limits a concern.
- **Additional Protocol 28 fixtures.** CAP-0086 sparse-map host functions
  have no fixture yet — the currently published Soroban SDK surface does
  not expose the host functionality the intended fixture would need. See
  [Protocol 28](./protocol-28.md). Track upstream SDK releases and add
  the fixture (in `ProtocolCanary-Fixtures`) once that changes.
- **More RPC compatibility assertions.** Only `getNetwork` has a fixture
  today; `getLatestLedger` is supported by `canary-rpc` but unused by any
  shipped fixture.

## Medium term

- **Protocol 29 support**, once its CAPs are finalized and released
  upstream — a new protocol pack entry, plus fixtures in
  `ProtocolCanary-Fixtures`, following the same process as Protocol 28.
  **No release currently supports Protocol 29 or any version beyond 28** —
  do not infer support from this roadmap entry.
- **Expanded Soroban compatibility assertions** beyond the current
  construct → simulate → result smoke fixture, covering more host
  functions as they stabilize.
- **Additional CI integrations** beyond GitHub Actions, if a real need
  arises, scoped as their own repository rather than bolted onto
  `ProtocolCanary-Action`.

## Explicitly out of scope for now

- A hosted backend, database, or web frontend — this is a CLI and CI tool.
- Remote/versioned fixture fetching (as opposed to a local checkout).
- Transaction submission of any kind.

## How to propose an addition

Open an issue describing the real upstream behavior (a CAP, an RPC
method, a host function) the work would cover. See
[Contributing](./contributing.md).
