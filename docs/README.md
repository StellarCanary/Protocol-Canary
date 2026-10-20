# Protocol Canary Documentation Index

This directory contains architectural specifications, fixture authoring contracts, protocol compatibility pack guides, and shared subsystem definitions for [`Protocol-Canary`](https://github.com/StellarCanary/Protocol-Canary).

---

## 1. Fixture Specifications & Authoring

Protocol Canary relies on declarative TOML fixtures to assert protocol compatibility across XDR, RPC, and Soroban surfaces.

- **[Fixture Loading Contract (`fixture-contract.md`)](fixture-contract.md)**  
  The formal interface defining where fixtures are loaded from, directory traversal, ID resolution, per-surface parsing boundaries, and compatibility rules.

- **[Soroban ScVal Fixture Specification & Authoring Guide (`fixtures/scval.md`)](fixtures/scval.md)**  
  Detailed authoring guide for Soroban Contract Values (`ScVal`), covering encoding rules for all supported variants (primitives, wide integers `u128`/`i128`/`u256`/`i256`, temporal types, hex/base64 bytes, strkey addresses, and recursive composite `vec`/`map` structures up to depth 8), contract invocation arguments (`[[args]]`), and low-level base64 XDR `type = "ScVal"` fixtures.

---

## 2. Core Architecture & Protocol Packs

- **[Architecture & Execution Topology (`architecture.md`)](architecture.md)**  
  System design, offline vs online boundaries, transaction simulation model, caching mechanisms, and external dependency guarantees.

- **[Protocol 28 Compatibility Pack (`protocol-28.md`)](protocol-28.md)**  
  Detailed coverage of the Protocol 28 release, including CAP-0083 (Transaction Set XDR), CAP-0085 (Soroban Host Functions), and live Testnet verification.

- **[Release Engineering Guide (`releasing.md`)](releasing.md)**  
  Procedures for cutting releases, generating deterministic verification checksums, updating version matrices, and publishing changelogs.

---

## 3. Shared Contracts (`docs/contracts/`)

The contracts defined in [`docs/contracts/`](contracts/README.md) establish shared rules between `Protocol-Canary`, [`ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures), and [`ProtocolCanary-Action`](https://github.com/StellarCanary/ProtocolCanary-Action):

- **[CF-01: Report Compatibility & Semantics](contracts/cf-01-report.md)**: JSON report structure and exit codes.
- **[CF-02: Fixture Registry & Pack Digest](contracts/cf-02-registry-and-digest.md)**: Canonical registry and SHA-256 pack fingerprinting.
- **[CF-03: Lockfile Specification](contracts/cf-03-lockfile.md)**: `.stellar-canary.lock` format and pin validation.
- **[CF-04: Verification Evidence](contracts/cf-04-verification-evidence.md)**: Live network capture records and provenance proofs.
- **[CF-05: Report Comparison & Diff](contracts/cf-05-report-comparison.md)**: Comparing runs across versions and environments.
- **[CF-06: Canonical Fixture Releases](contracts/cf-06-fixture-releases.md)**: Packaging and release distribution.
- **[CF-07: Project Roots & Detection](contracts/cf-07-project-roots-and-detection.md)**: Workspace detection and project capability scanning.
- **[CF-08: Viewer & GitHub Action Integration](contracts/cf-08-viewer-and-action.md)**: CI integration and report viewer.
- **[CA-01 through CA-04](contracts/README.md)**: RPC requests, fixtures inventory JSON, inspect plan JSON, and project fingerprinting.

---

## 4. Reporting Contracts

- **[JSON Report Contract (`json-report-contract.md`)](json-report-contract.md)**  
  The exact byte-level contract emitted when `stellar-canary check --json` or `stellar-canary report` is invoked.
