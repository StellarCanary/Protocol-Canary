# CF-03: StellarCanary lockfile

Status: frozen for implementation planning, contract version 1.
Updated with maintainer decisions D-04 (exit code 2 for a stale lock) and D-12 (engine-version mismatch is a warning in lock.warnings).
Authoritative for: `.stellar-canary.lock` version 1.

## 1. Purpose and existing behavior

No lockfile exists. Today a run is identified by flags and by whatever happens
to be in `--fixtures-dir`. Configuration comes from `.stellar-canary.toml`
(`canary-config`, `SUPPORTED_CONFIG_VERSION = 1`) and the CLI; precedence is CLI
flag, then config file, then built-in default (verified for `--protocol` in
`docs/fixture-contract.md` section 5 and `canary-config`). The CLI reads no
environment variables.

The lock pins **what is checked against**, so two machines run the same fixture
bytes. It does not pin network answers.

## 2. File

- Name `.stellar-canary.lock`, at the project root, next to
  `.stellar-canary.toml`.
- TOML, UTF-8, at most 64 KiB.
- Written only by an explicit command (`lock`, planned). `check` never creates
  or modifies it.

<!-- contract-example: lock-v1 -->
```toml
# Illustrative values only. Digests below are not real.
lockVersion = 1

[tool]
version = "0.1.1"

[fixtures]
protocol = 28
packDigest = "sha256:0000000000000000000000000000000000000000000000000000000000000000"
registryVersion = 1
source = "release"
revision = "0000000000000000000000000000000000000000"
```

Required: `lockVersion`, `fixtures.protocol`, `fixtures.packDigest`,
`fixtures.registryVersion`. Optional: `tool.version`, `fixtures.source`
(`"directory"` or `"release"`), `fixtures.revision` (informational only, never
compared).

## 3. Parser rules

1. `lockVersion` must be the integer `1`. Anything else is an error that names
   the found and supported versions.
2. Unknown keys and unknown tables are errors (unlike report consumers, which
   ignore unknown fields: a lock is a pin, and an ignored key could silently drop a
   pin, while a report is only a record). A lock is a pin; silently ignoring
   a key could silently drop a pin.
3. Duplicate keys are errors (the TOML parser already rejects them).
4. `packDigest` must match `^sha256:[0-9a-f]{64}$`.
5. `protocol` is a non-negative integer. `registryVersion` must be a version the
   running binary supports.
6. `tool.version` must be `MAJOR.MINOR.PATCH` with no leading `v`, the same
   syntax the Action accepts for its `version` input.
7. Files over 64 KiB, non-UTF-8 files and non-regular files (including
   symlinks) are rejected.

Serialization is deterministic: keys in the order shown, `\n` line endings,
trailing newline, no comments except the optional header the writer emits.

## 4. Precedence

From highest to lowest: explicit CLI flag, `.stellar-canary.toml`, lockfile,
built-in default.

The lock supplies identity, not settings, so it only conflicts on facts:

| Situation | Behavior |
|---|---|
| Lock present, no `--fixtures-dir` given, directory resolved by default | Verify the resolved directory against `packDigest`. |
| `--fixtures-dir` given | Still verified. A flag chooses a location; it does not waive the pin. |
| `--no-lock` given | Lock ignored entirely and the report records `lock.status = "absent"`. |
| Lock `protocol` differs from the effective target protocol | Stale. |
| Computed pack digest differs from `packDigest` | Stale. |
| `tool.version` present and differs from the running binary | Warning on stderr and recorded in `lock.warnings`; not a failure in version 1 (approved under D-12: the lock pins fixtures, not the engine, so it does not stop a different engine from running). |

Stale means: the run stops before executing anything, prints the expected and
computed digest, and exits with the configuration error code `2` (approved under D-04). A stale lock is never auto-refreshed.

Absent lock behaves exactly as today.

## 5. Fixture identity and reproducibility

Identity of the fixture set is `packDigest` as defined in CF-02, computed over the
directory actually loaded. Reproducible: same lock, same binary, same project
commit, same fixture bytes imply the same plan (which fixtures run, which are
skipped). Not reproducible and not claimed: results of RPC and Soroban
fixtures, which depend on live network state at run time.

Dirty git state: the lock never records the project commit. `git.isDirty` stays
in the report as today.

## 6. Writing safely

The writer creates a temporary file in the same directory, flushes, then renames
over `.stellar-canary.lock`. If another process holds a `.stellar-canary.lock.tmp`
or the target is a symlink, it fails instead of overwriting. Output is
byte-identical for identical inputs.

## 7. Security and trust boundaries

The lock is data in the consumer's repository and is trusted at the same level
as the rest of that repository. It cannot name a URL, a command or an
executable. A pull request that edits `packDigest` is the review point where a
new pack is accepted. The lock never contains credentials.

## 8. Examples of invalid locks (illustrative)

<!-- contract-example: lock-v1-invalid -->
```toml
lockVersion = 2
```

<!-- contract-example: lock-v1-invalid -->
```toml
lockVersion = 1
[fixtures]
protocol = 28
packDigest = "md5:abc"
registryVersion = 1
```

## 9. Validation and test requirements

- Round trip: parse then serialize equals original bytes for canonical input.
- Every parser rule has a rejecting test.
- Stale detection: changed fixture byte, added file, removed file, wrong
  protocol.
- Precedence tests: `--fixtures-dir` with a matching and a mismatching pack,
  `--no-lock`.
- Writer: concurrent writer test, symlink target test, byte-identical output.
- Detached HEAD and dirty tree do not change lock behavior.

## 10. Cross-repository dependencies

CF-02 (digest). CF-06 (release manifest supplies `packDigest` and `revision`).
Action reads the lock only to pass `--fixtures-dir` consistently and to
display the pin; it does not parse digests itself.

## 11. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CF-03-A Parser and serializer crate module with all rules | Protocol-Canary | CF-02 vectors for the digest format only |
| CF-03-B `lock` command that writes from a directory or a release manifest | Protocol-Canary | CF-03-A, CF-02-F |
| CF-03-C Verification during `check` and `--no-lock` | Protocol-Canary | CF-03-A, CF-02-F |
| CF-03-D `lock verify` command, read only | Protocol-Canary | CF-03-A, CF-02-F |
| CF-03-E Atomic writer and concurrency tests | Protocol-Canary | CF-03-A |
| CF-03-F Action reads the lock and reports its status | ProtocolCanary-Action | CF-03-C |
