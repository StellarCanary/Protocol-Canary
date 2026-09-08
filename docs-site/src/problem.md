# What Problem Does It Solve?

Think of Protocol Canary as an early-warning system: like the caged birds
once carried into mines to detect dangerous gas before it reached the
miners, it is meant to detect a compatibility problem before it reaches
production — not to guarantee that no problem exists.

## The real problem

Stellar protocols evolve. A protocol upgrade can add new XDR types (for
example, Protocol 28's `CAP-0083` `StellarValue` case), change RPC
response shapes, or add new Soroban host behavior. Applications and
infrastructure depend on the current shape of all three surfaces, often
implicitly — a hand-written XDR decoder, an assumption about an RPC
field, a Soroban call that happens to work today.

Normal unit tests primarily exercise application code against mocked or
assumed protocol behavior. They do not, by themselves, test the boundary
between that application code and the real, current behavior of the
Stellar network and its official libraries. That boundary is exactly what
changes when a protocol upgrades.

## What Protocol Canary does about it

It makes that compatibility boundary **explicit** and **repeatable**:

- Explicit, because every check is a declared fixture — a specific XDR
  value, RPC assertion, or Soroban call, with a source reference (a CAP
  number or official spec) — not an implicit assumption baked into
  application code.
- Repeatable, because the same fixture pack and the same CLI produce the
  same result locally and in CI, against the real `stellar-xdr` crate and
  a real RPC endpoint, rather than a mock.

## What it does not claim

- It does not claim that every protocol change causes breakage — most
  fixtures simply keep passing.
- It does not prove an arbitrary application is compatible with a
  protocol version in any general sense. A passing run means the fixtures
  that are currently implemented for the target protocol passed against
  your configured dependencies and RPC endpoint — see
  [Limitations](./limitations.md).
- It does not replace testing against a real testnet or mainnet
  deployment.
