# Security Policy

## Scope

This policy covers the `Protocol-Canary` CLI and the crates in this
workspace. It does not cover the separate `ProtocolCanary-Fixtures` or
`ProtocolCanary-Action` repositories, which have their own security
policies.

## Supported versions

Only the latest released `0.x` minor version receives security fixes while
the project is pre-1.0. There is no long-term support branch yet.

## No private-key policy

Protocol Canary never asks for, stores, or transmits a secret key, seed
phrase, or private key. No compatibility check in this repository requires
signing authority over a real account.

## No transaction-submission policy

In this MVP, Protocol Canary only performs read operations (for example,
`getNetwork`, `getLatestLedger`) and Soroban **simulation** against a
configured RPC endpoint. It never submits a transaction to a network.

## Custody statement

Protocol Canary does not custody funds, does not hold account credentials,
and has no wallet functionality.

## Dependency expectations

Dependencies are chosen deliberately (see `CONTRIBUTING.md`) and kept to
what the compatibility checks actually require. XDR-bearing types come from
the official `stellar-xdr` crate rather than a hand-rolled parser.

## Audit status

No formal third-party security audit has been performed on this
repository. Confidence in the claims above comes from the design itself
(no key material path exists in the code) and from the automated test
suite, not from an external review.

## Reporting a vulnerability

If you find a security issue in this repository, please open a private
report via GitHub's "Report a vulnerability" feature on this repository, or
contact the maintainers directly rather than filing a public issue. Please
include:

- a description of the issue and its impact;
- steps to reproduce;
- the affected version or commit.

We will acknowledge reports and work with you on a fix and disclosure
timeline before any public write-up.
