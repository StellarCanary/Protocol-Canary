# Reproducing a Protocol Canary Run

This guide provides step-by-step procedures for deterministically reproducing a previous Stellar Protocol Canary run using the project lockfile (`.stellar-canary.lock`), pinned fixture pack digest, and git commit revision.

Reproducibility allows auditors, maintainers, and CI workflows to independently verify compatibility results and assert that two runs over identical inputs evaluate to identical outcomes.

---

## Overview

A deterministic run reproduction depends on four pinned inputs:

1. **Tool binary**: The exact version of `stellar-canary` built with `--locked` dependencies.
2. **Project revision**: The exact Git commit hash of the project under test.
3. **Fixture pack**: The immutable fixture collection whose SHA-256 directory digest matches `fixtures.packDigest` in `.stellar-canary.lock`.
4. **Execution mode**: Offline execution (`--offline`) for strictly reproducible local assertions that do not depend on external network state.

---

## Step 1: Check Out the Project Revision

Protocol Canary reports record the checked-out commit in `git.commit`. Check out the exact revision of the repository to reproduce:

```bash
git checkout <COMMIT_HASH>
```

Verify that the working tree is clean to avoid dirty state variations (`git.isDirty`):

```bash
git status
```

---

## Step 2: Validate the Lockfile

The project root contains `.stellar-canary.lock`, which pins the compatibility target and fixture pack:

```toml
lockVersion = 1

[tool]
version = "0.2.0"

[fixtures]
protocol = 28
packDigest = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
registryVersion = 1
source = "release"
revision = "abc1234"
```

The lockfile defines the authoritative pin for compatibility checks:

- `lockVersion`: Format version of the lockfile (currently `1`).
- `tool.version`: Semantic version of the Canary CLI used to create the lock.
- `fixtures.protocol`: The Stellar protocol version under test (e.g., `28`).
- `fixtures.packDigest`: SHA-256 hash computed over all fixture files in the pack (per CF-02).
- `fixtures.registryVersion`: Fixture registry schema version.

---

## Step 3: Obtain and Verify the Pinned Fixture Pack

Download or check out the fixture pack corresponding to the target protocol and pack digest from the [ProtocolCanary-Fixtures](https://github.com/StellarCanary/ProtocolCanary-Fixtures) repository.

Verify that the downloaded fixture pack matches the pinned `packDigest` using `lock verify`:

```bash
stellar-canary lock verify --fixtures-dir path/to/fixtures
```

### Verification Behavior

- **Exit code 0**: The fixture directory computes to the exact `packDigest` in `.stellar-canary.lock`.
- **Exit code 1**: Digest mismatch, missing lockfile, or corrupted fixture files. The CLI prints diagnostic details and refuses execution.

---

## Step 4: Install the CLI with `--locked`

Install or build the exact Canary CLI version matching `tool.version`, enforcing locked Cargo dependencies:

```bash
cargo install --git https://github.com/StellarCanary/Protocol-Canary --tag v0.2.0 --locked
```

Alternatively, from a local source checkout:

```bash
cargo install --path crates/canary-cli --locked
```

---

## Step 5: Execute in Offline Mode

Run the compatibility check using `--offline` to ensure deterministic execution without contacting live RPC or Soroban endpoints:

```bash
stellar-canary check --offline --fixtures-dir path/to/fixtures --format json --output report.json
```

Offline execution runs deterministic XDR decoding and encoding assertions. Network-dependent RPC and Soroban fixtures are skipped with the machine-readable skip code `OFFLINE`, ensuring that runs performed at different times or from behind firewalls produce identical results.

---

## Step 6: Diff Verification and Report Comparison

### Viewing the Rendered Report

Inspect the newly generated report in terminal format without re-running tests:

```bash
stellar-canary report report.json --format terminal
```

Or render it as Markdown:

```bash
stellar-canary report report.json --format markdown
```

### Diffing Against a Reference Report

Compare the generated JSON report against the reference report from the original run:

```bash
diff -u reference-report.json report.json
```

### Field Stability & Hash Matching

When comparing reports, note the distinction between stable and environment-dependent fields:

- **Strictly deterministic fields**:
  - `schemaVersion`, `targetProtocol`, `status`
  - `counts` (`total`, `passed`, `failed`, `warnings`, `errors`, `skipped`)
  - `results[].testId`, `results[].surface`, `results[].status`, `results[].summary`
  - `skipped[].fixtureId`, `skipped[].surface`, `skipped[].code`
  - `project.type`, `project.name`
- **Dynamic fields**:
  - `results[].durationMs`: Wall-clock execution time for individual fixtures.
  - `git.branch`: Branch name when detached or rebased.

To compare only deterministic content, normalize or ignore `durationMs`:

```bash
jq 'del(.results[].durationMs)' reference-report.json > ref-normalized.json
jq 'del(.results[].durationMs)' report.json > rep-normalized.json
diff -u ref-normalized.json rep-normalized.json
```

If the normalized files match byte-for-byte, the reproduction is confirmed.
