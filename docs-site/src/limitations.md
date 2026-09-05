# Limitations

Stated plainly, not hidden:

- **Current fixture coverage is not complete Protocol 28 coverage.** Five
  fixtures exist today, across three CAPs' surfaces. See
  [Protocol 28](./protocol-28.md) for exactly what is and isn't checked.
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
- **`CacheStore` exists but is not currently wired into the primary check
  flow.** The type is implemented and unit-tested in `canary-core`, but
  `check` calls RPC/Soroban fresh on every run rather than reusing a
  prior cached result. See [Architecture](./architecture.md#stateful-vs-ephemeral-components).
- **No formal third-party security audit has been performed** on any of
  the three repositories. See [Security](./security.md#audit-status).
- **Mainnet has not been separately verified.** Every fixture in the
  current pack was verified against Testnet only; running against
  mainnet requires explicit `--network mainnet --rpc-url <endpoint>`
  configuration.
- **Only Protocol 28 is supported.** There is no Protocol 29 (or earlier,
  beyond an unpopulated `protocol-27/` placeholder pack) support in any
  released version.
