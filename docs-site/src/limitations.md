# Limitations

Stated plainly, not hidden:

- **Fixture coverage is not complete Protocol 28 coverage.** The
  `protocol-28` pack at revision
  [`e9d098d`](https://github.com/StellarCanary/ProtocolCanary-Fixtures/tree/e9d098df7931a9b94b183c6af807a58336b7b322/protocol-28)
  holds 93 fixture definitions (86 XDR, 2 RPC, 5 Soroban). That is pack
  inventory, not checks run: a project executes only the fixtures its
  configuration and capabilities select, and fixture counts say nothing
  about live network behavior. See [Protocol 28](./protocol-28.md) for
  exactly what is and isn't checked.
- **CAP-0086 currently lacks a fixture.** This is a documented, deliberate
  gap — the published `soroban-sdk` doesn't yet expose the relevant host
  functions at a stable API surface, and this project does not fabricate
  a fixture it cannot independently verify. See [Roadmap](./roadmap.md).
- **Protocol Canary does not certify arbitrary applications.** A passing
  run means the fixtures currently implemented for the target protocol
  passed against your configured dependencies and RPC endpoint — it is
  not a guarantee that an application cannot break in some other way, and
  it does not replace testing against a real testnet or mainnet
  deployment.
- **Live RPC checks depend on network availability.** The RPC and Soroban
  surfaces are not offline — see [Network Troubleshooting](./troubleshooting.md#network-troubleshooting).
  Only the XDR surface runs with no network call.
- **The tool does not submit transactions as part of normal Soroban
  simulation checks.** Every Soroban check builds and simulates an
  unsigned transaction; nothing in a normal `check` run mutates the
  target network.
- **The current release does not publish prebuilt binaries.** Every
  documented install path builds from source via `cargo install`. See
  [Releases](./releases.md).
- **`--live-cache-ttl` replays recorded network answers.** It is off by
  default. When set, a cached RPC or Soroban result is served for up to that
  many seconds and marked `"source": "cache"`; it says nothing about what the
  network does now. See [`check`](./cli/check.md#result-cache).
- **No formal third-party security audit has been performed** on any of
  the three repositories. See [Security](./security.md#audit-status).
- **Mainnet has not been separately verified.** Every fixture in the
  current pack was verified against Testnet only; running against
  mainnet requires explicit `--network mainnet --rpc-url <endpoint>`
  configuration.
- **Only Protocol 28 is supported.** There is no Protocol 29 (or earlier,
  beyond an unpopulated `protocol-27/` placeholder pack) support in any
  released version.
