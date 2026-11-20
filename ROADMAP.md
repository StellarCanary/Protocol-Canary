# Roadmap

Practical, scoped work that fits the current architecture. This is not a
schedule with committed dates — items move up when someone (maintainer or
contributor) picks them up.

## Already shipped

- **A static report viewer.** [`viewer/`](viewer/README.md) reads a chosen
  JSON report in the tab, makes no network request, and stores only a theme
  preference. It is published at `/viewer/` beside the docs; it is not a
  hosted service. See [CF-08](docs/contracts/cf-08-viewer-and-action.md).
- **Protocol 29 fixture plumbing.** Engine `v0.3.0` accepts the
  `get-version-info` RPC fixture method and its `protocolVersion` field, so
  the published Protocol 29 pack can execute. It also accepts more read-only
  RPC methods and `decode-json-equals` XDR fixtures. That is method support,
  not a general Protocol 29 compatibility claim — see
  [CHANGELOG.md](CHANGELOG.md) and
  [what a release does not do](docs/releasing.md#what-a-release-does-not-do).
- **Protocol 30 offline XDR vectors.** CI runs the CAP-0084/CAP-0088 offline
  XDR fixtures from the fixtures repo's `protocol-30/` pack, with its RPC
  identity fixture skipped because that run is offline
  ([`ci.yml`](.github/workflows/ci.yml)). The pack's Testnet RPC identity
  fixture is a candidate observation, not full Protocol 30 support; see
  [CF-04 section 5](docs/contracts/cf-04-verification-evidence.md#5-protocol-29-what-is-established-and-what-is-not).

## Near term

- **Additional Protocol 28 fixtures.** CAP-0086 sparse-map host functions
  still have no fixture — the published Soroban SDK surface does not expose
  the host functionality the intended fixture would need. See
  [`docs/protocol-28.md`](docs/protocol-28.md). Track upstream SDK releases
  and add the fixture (in `ProtocolCanary-Fixtures`) once that changes.
- **Evidence-backed fixtures for the other RPC methods.** The engine parses
  ten read-only methods, but the shipped packs exercise `getNetwork`
  (Protocol 28) and `get-version-info` (Protocol 29). The open work is a
  fixture for another method with a real capture and verification record,
  not more engine methods. See
  [CF-04](docs/contracts/cf-04-verification-evidence.md).
- **Wider Protocol 30 coverage.** The offline XDR vectors and the Testnet
  identity observation do not establish compatibility. More Protocol 30
  fixtures need authoritative sources and live verification records before
  any broader claim is made.

## Medium term

- **Expanded Soroban compatibility assertions** beyond the current
  construct → simulate → result smoke fixture, covering more host
  functions as they stabilize.
- **Additional CI integrations** beyond GitHub Actions, if a real need
  arises (e.g. GitLab CI), scoped as their own repository rather than
  bolted onto `ProtocolCanary-Action`.

## Current boundaries

- This is a CLI and CI tool: the CLI decides compatibility and the viewer
  only displays a report. There is no hosted backend or database.
- The live checks in the shipped packs were verified against Testnet only;
  mainnet is not separately verified. A target-versus-observed protocol
  mismatch (for example a Protocol 28 run against a network reporting 29)
  stays visible and is never treated as compatibility evidence.
- Remote/versioned fixture fetching is still out of scope: consumers check
  out a pack locally and pass `--fixtures-dir`. See
  [`docs/fixture-contract.md`](docs/fixture-contract.md).
- No transaction submission, signing, or secret-key handling.

## How to propose an addition

Open an issue describing the real upstream behavior (a CAP, an RPC method,
a host function) the work would cover. See [CONTRIBUTING.md](CONTRIBUTING.md).
