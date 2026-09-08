# Security

This page summarizes the security posture documented across all three
repositories' own `SECURITY.md` files, which remain authoritative:
[`Protocol-Canary`](https://github.com/StellarCanary/Protocol-Canary/blob/main/SECURITY.md),
[`ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures/blob/main/SECURITY.md),
[`ProtocolCanary-Action`](https://github.com/StellarCanary/ProtocolCanary-Action/blob/main/SECURITY.md).

## What Protocol Canary executes

- **The CLI** decodes/encodes XDR against the official `stellar-xdr`
  crate, makes read-only RPC calls (`getNetwork`, `getLatestLedger`), and
  builds/simulates unsigned Soroban transactions
  (`simulateTransaction`).
- **Fixtures are declarative TOML data, never executable code.** No field
  in the fixture schema runs a shell command, script, or arbitrary code,
  and none will be added — a pull request introducing one is rejected
  regardless of stated purpose.
- **The Action** never executes a shell string built from fixture data,
  repository configuration, PR body/title, issue text, or commit
  messages. Arguments are always passed to the Canary process as an array
  (`child_process.spawn(binary, [...args])`), never through a shell.

## What it does not execute

- No transaction submission, anywhere in the system. Every network
  interaction is read-only or simulation-only.
- No arbitrary code from a fixture file.
- No downloaded script executed by the Action — the toolchain performing
  the build is `cargo`, already present on the runner.

## Network behavior

- Only outbound, read/simulate calls to a configured Stellar RPC endpoint
  (Testnet by default). Nothing in a normal `check` run mutates the
  target network.
- A failure to reach the endpoint is reported as an execution error
  ([exit code `3`](./exit-codes.md)), never silently ignored and never
  misreported as a compatibility result.

## Private-key behavior

No component in this system ever reads, stores, or transmits a private
key, seed phrase, or any other signing credential. The Soroban surface
builds and simulates **unsigned** transactions only. This has been
verified directly against the source: the only `std::env` calls in
`Protocol-Canary` are `temp_dir()`/`current_dir()` — there is no key
material code path.

## Fixture trust boundary

`ProtocolCanary-Fixtures` fixtures — and any third-party fixture
directory — must be treated as **untrusted input** by any consumer,
including `Protocol-Canary` itself. The schema has no executable field.
`decode-failure` fixtures deliberately feed malformed input to the XDR
decoder to prove it rejects bad input correctly, rather than crashing or
silently accepting it.

## Action trust boundary

Pull request source code is treated as untrusted by the Action. It never
interpolates PR-controlled text into a shell command, and it enforces a
configurable timeout (`timeout-minutes`, default `15`) that forwards
workflow cancellation signals to the child process, so a hung or
malicious process cannot outlive the job.

The Action's minimum required permission is `contents: read`; it does not
call the GitHub API on your repository at all for its core function (it
does call the public, unauthenticated GitHub REST API against
`StellarCanary/Protocol-Canary` itself, to resolve a release tag to a
commit before installing it).

## Report handling

The JSON report the CLI produces is plain data (see [JSON Report](./json-report.md)).
The Action parses it and never re-executes anything based on its content
beyond deciding pass/fail and rendering text.

## Reporting a vulnerability

Each repository documents the same private path: open a private report
via GitHub's "Report a vulnerability" feature on that repository, or
contact the maintainers directly, rather than filing a public issue.
Include a description of the issue and its impact, steps to reproduce,
and the affected version or commit. Reports are acknowledged and worked
through a fix and disclosure timeline before any public write-up.

## Audit status

**No formal third-party security audit has been performed on any of the
three repositories.** Confidence in the claims above comes from the
design itself (no key-material code path exists) and each repository's
own automated test suite — not from an external review. This project does
not claim, and has never claimed, any security certification.
