# FAQ

## Why not just use unit tests?

Unit tests primarily exercise your application code against mocked or
assumed protocol behavior. They don't, by themselves, test the boundary
between your code and the real, current behavior of the Stellar network
and its official libraries — which is exactly what changes when a
protocol upgrades. See [What Problem Does It Solve?](./problem.md).

## Does Canary send transactions?

No. Every Soroban check builds and simulates an **unsigned** transaction
via `simulateTransaction`. Nothing in a normal `check` run submits a
transaction to any network.

## Does Canary need private keys?

No. No component in this system ever reads, stores, or transmits a
private key or seed phrase — verified directly against the source. See
[Security](./security.md#private-key-behavior).

## Does Canary require Docker?

No. It's a Rust CLI you `cargo install`, plus a Node-based GitHub Action
for CI. Neither requires Docker.

## Does Canary require a server?

No. There is no hosted backend, no database, and no persistent server
anywhere in this system. See [Architecture](./architecture.md).

## What happens if RPC is unavailable?

The affected checks report an **execution error** (`status: error`, exit
code `3`), not a compatibility failure. Offline (XDR) checks in the same
run are unaffected and still report their real result. See
[Network Troubleshooting](./troubleshooting.md#network-troubleshooting).

## Does 5/5 mean full Protocol 28 support?

**No.** It means the five fixtures currently implemented for Protocol 28
passed. CAP-0086 has no fixture yet, and CAP-0085's fixtures test wire
encoding, not a full deployed contract fleet. See
[Protocol 28](./protocol-28.md) for exactly what is and isn't covered.

## Why are fixtures separate from the engine?

So fixtures — public, declarative TOML data consumed by CI pipelines —
stay a clean trust boundary: no field executes code, and the fixture pack
can version independently of the CLI. See [Fixtures](./fixtures-guide.md#why-fixtures-are-separate-from-the-engine).

## How do I use Canary in GitHub Actions?

```yaml
- uses: actions/checkout@v4
- uses: StellarCanary/ProtocolCanary-Action@v1
  with:
    protocol: "28"
```

See [GitHub Action](./github-action.md) for inputs, outputs, and a
complete workflow checking the real Protocol 28 fixture pack.

## How do I add support for another protocol behavior?

If it's a new compatibility *assertion* for an existing surface, add a
fixture — see [Authoring a Fixture](./fixture-authoring.md). If it
requires new engine capability (a new surface, a new XDR type the CLI
doesn't yet decode), that's a change to `Protocol-Canary` itself — see
[Contributing](./contributing.md#core-engine--protocol-canary). Either
way, it must cite a real, verifiable upstream source — this project does
not add speculative or guessed behavior.
