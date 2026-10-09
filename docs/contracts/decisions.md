# Decision table for maintainer review

Prepared 2026-10-09. **Every row is a proposal.** Only D-02 and D-09 are approved
(recorded in [`README.md`](README.md)). Approval is recorded by a pull request that
edits the "Approved" table there; nothing in this file changes behavior.

"Security-sensitive" means the choice changes what an attacker or a mistake can do,
so it should be approved explicitly rather than inherited from a default.

| ID | Proposed direction | Status | Security-sensitive | Technical review (what was checked and what it found) | Blocker or evidence needed |
|---|---|---|---|---|---|
| D-01 | Optional additive report fields stay within `schemaVersion` 1; a breaking change needs version 2. | Proposed | No | Consistent with CF-01 and with the one field already added (`results[].source`). One consequence to state in the decision: adding a value to a closed enum (`status`, `surface`) is breaking, because both the engine and the Action reject unknown values on purpose. | None |
| D-03 | Commit the canonical registry in `ProtocolCanary-Fixtures` and verify freshness in CI. | Proposed | No | A committed, generated file that every fixture-changing pull request must regenerate is a merge-conflict hot spot during a Wave. Mitigation recorded in CF-02: one registry per protocol pack (so only PRs to the same pack conflict), and the freshness check prints the regenerate command. With 7 fixtures today the cost is small. | None |
| D-04 | A stale lockfile fails with exit code 2 and a clear diagnostic. | Proposed | Low | Same code as the empty-plan refusal (D-02); the message is the discriminator. A caller that needs to tell the two apart cannot do so by code. | None |
| D-05 | Default freshness window for live verification evidence is 30 days. | Proposed | No | Applies to CF-04 evidence only. Unrelated to the result cache, which has no default window. No evidence records or scheduled verification exist yet, so the number has no effect until FX work lands. | None |
| D-06 | Bounded archives, scans and report sizes, with the numbers in CF-05, CF-06, CF-07 and CA-01 / CA-04. | Proposed | Yes (resource limits) | Measured today: the Protocol 28 pack is 8 files and 17,102 bytes (largest file 3,450 bytes); a Mainnet `getLatestLedger` response was about 1.28 MB on 2026-10-09. The proposed archive limits (2,000 entries, 16 MiB total, 1 MiB per file) leave roughly 1,000 times the pack's size; the maintainers may prefer tighter archive numbers (for example 1 MiB total, 256 KiB per file) with an explicit way to raise them. The RPC response cap (16 MiB) is about 12 times the largest observed response. | Maintainer chooses the numbers |
| D-07 | Defer release signatures beyond checksums; never present a checksum as proof of who published a release. | Proposed | Yes | `docs/releasing.md` already says a checksum from the same page protects against corruption only. Residual risk if deferred: a compromised release page can serve a matching binary and checksum. The lockfile pin (a digest reviewed in the consumer's repository) is the only independent anchor for fixture packs, not for the engine binary. | Explicit acceptance of the residual risk |
| D-08 | No silent upward project-root discovery; the current directory or `--project-root` only. | Proposed | Low | Matches CF-07 and the design of `--project-root` (E-010). | None |
| D-08b | Host the static viewer beside the existing documentation. | Proposed | Yes (origin) | A GitHub Pages project site is served from `stellarcanary.github.io`, an origin shared with every other Pages site of the organization, so storage and cookies are shared across them. The viewer stores only a theme preference and never reports, and uses a meta CSP with `connect-src 'none'`. The decision should state that no other Pages site in the organization may read or write the viewer's storage keys, or the viewer should use a dedicated path prefix for its keys. | Maintainer accepts the shared origin |
| D-10 | Diagnostics artifacts are opt-in and off by default. | Proposed | Yes | Artifacts on a public repository are readable by anyone who can read the repository, and pattern-based redaction is best effort. Opt-in with default off is the conservative choice. Draft issue A-003 is written to that default. | Approval before A-003 is published |
| D-11 | **Unresolved.** The download redirect policy must be verified against real release hosts. | **Blocked** | Yes | Observed 2026-10-09 on a third-party release (not a StellarCanary one): `github.com/.../releases/download/...` answers `302` to `release-assets.githubusercontent.com`, which answers `200`. No StellarCanary release has an uploaded asset yet (`v0.1.1` of both the engine and the Action has none, and the Fixtures `protocol-28` release has none), so no StellarCanary release host has been observed. | The first StellarCanary release with an asset (engine 0.2.0 with the multi-platform job, or a Fixtures pack release); then record each hop with `curl -sI` and test the allow-list against it |
| D-12 | An engine-version mismatch under lockfile v1 produces a prominent warning. | Proposed | Low | The Action shows engine stderr only when a run fails, so a stderr-only warning is invisible on success. "Prominent" therefore needs a field in the report. CF-03 now reserves `lock.warnings`. | None beyond the field |

## Inconsistencies found and resolved in the contracts

These are corrections to text, not approvals.

1. CF-02 now names the D-03 conflict risk and the per-pack mitigation.
2. CF-03 and CF-01 now reserve `lock.warnings` so D-12 can be visible to consumers.
3. CF-08 now records the shared Pages origin next to D-08b.
4. CF-06 and D-06 now cite the measured pack and response sizes next to the proposed limits.
