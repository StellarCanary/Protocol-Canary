# CA-04: Project fingerprint

Status: frozen for implementation planning, contract version 1.
D-06 is approved (the per-file size limit in section 3, 1 MiB).
Authoritative for: a reproducible, path-free digest of the project inputs the
tool actually inspected, and what it can and cannot be used for.
Depends on: CF-07 (which files are inspected).

## 1. Purpose and existing behavior

Today a run identifies its project by `project.name` (the directory name) and the
Git context (`commit`, `branch`, `isDirty`). Neither says what the detector
looked at. The directory name is not unique or stable, a clean commit says
nothing about a monorepo member, and a dirty flag does not say *what* is dirty.
The result cache (RH-01) therefore keys on a conservative Git-based string.

A fingerprint answers one question: "were these the same inspected inputs?" It
does not answer "is this the same source tree", and it is not a security
identity.

What exists in `0.1.1` and informs the design (checked on `main` at `1ff7908`):
detection reads `Cargo.toml` and `package.json` in the root only; it checks
whether `stellar.toml` is a file; it looks for a `.wasm` file up to two levels
below the root (and in four known Cargo output directories), skipping `.git`,
`node_modules` and `target`; the explicit `[project].type` setting overrides the
detected type. `ProjectContext.root` is absolute and is not part of any report.

## 2. What the fingerprint covers

The fingerprint is a digest over the **inputs the detector read** and the
**results it derived from them**, for one selected project root (CF-07).

| Input | Included as |
|---|---|
| Each manifest file the detector read (CF-07 tier 1 and tier 2: `Cargo.toml`, `package.json`, member manifests, `go.mod`, `pyproject.toml`, `requirements*.txt`, `pom.xml`, Gradle files, `pnpm-workspace.yaml`) | `manifest`, SHA-256 of the raw bytes, byte length, path relative to the selected root |
| Each manifest the detector tried to read and could not (unreadable, too large, a symbolic link) | `unreadable`, path relative to the root, reason code |
| `stellar.toml` | `signal stellar-toml 1` or `0` (the detector only tests that it is a file) |
| Each WASM artifact the detector found | `artifact`, path relative to the root, byte length, and SHA-256 when the file is at most 1 MiB (Soroban contracts are far below that; larger files are recorded by length only, marked `-`) |
| The detected project type and the sorted capability list | `type`, `capability` lines. They are derived from the lines above, but are included so that a change in the detector's *logic* changes the digest |
| The explicit `[project].type` setting from configuration | `type-setting`, `auto` or the explicit type |
| A detector revision number | `detector`, an integer in the engine that is incremented whenever detection could give a different answer for the same bytes |

## 3. What it does not cover

- Source code, tests, documentation, build scripts, or any file the detector did
  not read. Adding a README or editing a `.rs` file does not change it.
- Dependency versions that are not written in a read manifest. `Cargo.lock`,
  `package-lock.json` and similar are not read and not hashed.
- The Git commit, branch or dirty flag. They stay separate report fields. The
  same manifests at two commits give the same fingerprint (that is the point for
  caching), and an edit to a manifest in a dirty tree changes it.
- Configuration other than the project type setting: protocol, enabled surfaces
  and policy are run inputs recorded elsewhere.
- Anything about the network.

A symbolic link met during the scan is never followed and is recorded as
`unreadable`, reason `symlink`. Files above the per-manifest size limit
(1 MiB, approved under D-06) are `unreadable`, reason `too-large`. A file whose path is
not valid UTF-8 cannot be normalized; it is not hashed, a `non-utf8-path` line
with only a counter is added, and the digest still changes if their number does.

## 4. Construction

The digest input is this text, UTF-8 with `\n` line ends:

```text
canary-project-fingerprint/1
detector <integer>
type-setting <auto|explicit-type>
type <detected-type>
capability <name>            one line per capability, sorted by name
signal stellar-toml <0|1>
manifest <sha256 hex> <byte length> <relative path>
unreadable <reason> <relative path>
artifact <sha256 hex or -> <byte length> <relative path>
non-utf8-path <count>
```

Lines of each kind are sorted by the relative path compared as UTF-8 bytes; the
kinds appear in the order shown. A path is relative to the selected root, uses `/`
on every platform, has no `.` or `..` segment, and never begins with `/`. The
fingerprint is `sha256:` followed by the lowercase hex SHA-256 of that text.

Hashing is over raw bytes with no newline or encoding normalization. A checkout
that rewrites line endings produces a different fingerprint, because the bytes
are different; the Fixtures repository handles the same issue with a
`.gitattributes` rule (CF-02), and a consumer repository that cares can do the same.

Consequences of the construction:

- **Path independence.** The selected root's own location, its name, and its
  position inside a monorepo do not appear. Copying a project to another
  directory, or checking it out on another machine, gives the same fingerprint if
  the read bytes are the same.
- **Order independence.** File system enumeration order does not matter.
- **Monorepos.** There is one fingerprint per selected root. A workspace member
  selected with `--project-root` is fingerprinted from the files inside it; the
  detector never reads outside the selected root (CF-07), so the workspace root
  manifest is not part of a member's fingerprint. Two identical member packages in
  different directories have the same fingerprint, by design; the fingerprint says
  nothing about *which* directory.
- **Dirty worktrees.** Uncommitted and untracked manifests are read like any
  other, so they are covered. Uncommitted changes to files the detector does not
  read are not, and cannot affect detection.

## 5. Output

Within `inspect --format json` (CA-03):

```json
"fingerprint": { "fingerprintVersion": 1, "detector": 1, "digest": "sha256:<64 hex>", "inputs": 2 }
```

`inputs` counts the `manifest`, `unreadable` and `artifact` lines. With
`--verbose` the document also carries `inputList`, an array of
`{ "kind", "path", "sha256", "bytes" }` in digest order (`sha256` absent for
`unreadable` or length-only artifacts), so a mismatch can be diagnosed by
comparing lists. Without `--verbose` no path appears anywhere in the output.

The fingerprint is not added to the `check` report in version 1. If a later
contract adds it, it is an optional additive field.

## 6. What it can and cannot be used for

Can: tell whether the inspected inputs changed between two runs; key a local
cache more precisely than a Git string; show in an issue that two machines
inspected identical inputs, without sharing paths.

Cannot:

- **Reproduce the project.** The digest is one-way. To reproduce it you need the
  same manifest bytes, the same small WASM bytes and a tool with the same
  detector revision.
- **Prove the source is the same**, only that the inspected files are.
- **Be treated as secret or anonymous.** A manifest with common content (a minimal
  `Cargo.toml`) hashes to a value anyone can compute, so a digest can confirm a
  guess about a file. It reveals nothing about files that were not read. Do not
  use it as an access credential or as proof of ownership.
- **Stay stable across detector changes.** A new release that changes what is
  detected changes `detector` and therefore the digest, on purpose. Compare
  fingerprints only between runs of the same `fingerprintVersion` and the same
  detector revision, both of which the `fingerprint` object shows.

## 7. Failure behavior

Inspection never fails because of the fingerprint. An unreadable or oversized
file is recorded as `unreadable` (it changes the digest) and a diagnostic is
added to the CF-07 evidence. A root that cannot be listed at all is a
configuration error (exit 2), as for detection.

## 8. Security and trust boundaries

The scan is the CF-07 scan: no execution, no network, no following links, bounded
file count, directory count and file size. The fingerprint code reads only files
the detector reads, plus WASM files for hashing, and refuses anything larger than
the limits. Output carries no absolute path; relative paths appear only with
`--verbose` and only for files inside the selected root.

## 9. Validation and test requirements

- A **conformance vector set** (small directory trees plus expected digests)
  committed as test data, so any reimplementation can check itself, in the style
  of the CF-02 vectors.
- Path independence: the same tree in two different temporary directories gives
  one digest.
- Order independence: create the files in two different orders.
- Sensitivity: change one byte of a manifest; add a manifest; remove one; change a
  small WASM by one byte; add `stellar.toml`; change the type setting. Each
  changes the digest.
- Insensitivity: add a README, add a source file, touch a lockfile, change the
  Git commit without changing a manifest. None changes the digest.
- Line endings: the same manifest with CRLF and with LF gives different digests
  (documented behavior, asserted).
- Symbolic link and oversized manifest: recorded as `unreadable`, digest differs
  from the tree without them, scan does not follow the link (Unix test).
- Non-UTF-8 name: counted, not hashed (Unix test).
- Monorepo: two member roots of one repository give independent fingerprints;
  identical members give equal ones.
- The same tree on Linux, macOS and Windows gives the same digest.
- `detector` revision change changes the digest (a test pins the current value so
  a detector change cannot ship without touching it).

## 10. Cross-repository dependencies

CF-07 (the set of inspected files and the root rule), CA-03 (the output). The
Action may display a fingerprint from `inspect` but does not compute one. The
Fixtures repository is unaffected.

## 11. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CA-04-A Digest construction over the inspected inputs, with the detector revision constant | Protocol-Canary | none for Cargo and `package.json`; later tiers add their manifests as CF-07 units land |
| CA-04-B Conformance vectors and the sensitivity and insensitivity tests | Protocol-Canary | CA-04-A |
| CA-04-C Cross-platform CI run of the vectors | Protocol-Canary | CA-04-B |
| CA-04-D `inspect` output and the `--verbose` input list | Protocol-Canary | CA-04-A, CA-03-A |
| CA-04-E Use the fingerprint in the result-cache key in place of the Git string | Protocol-Canary | CA-04-A, and a maintainer decision that cached results depend on detection only |
