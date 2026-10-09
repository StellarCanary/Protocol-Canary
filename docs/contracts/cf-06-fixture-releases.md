# CF-06: Canonical fixture releases

Status: frozen for implementation planning, contract version 1.
Pending maintainer decisions: D-06 (extraction limits), D-07 (signing beyond
checksums).
Authoritative for: fixture-pack release artifacts, manifest, immutability,
checksums, archive verification, safe extraction and compatibility.

## 1. Purpose and existing behavior

The only released pack is the git tag and GitHub release `protocol-28` in
`ProtocolCanary-Fixtures` (annotated tag object `285d7fc`, commit `75ec2c2`,
created 2026-09-14). It has no uploaded assets, no checksum and no manifest. Its
release text describes 5 fixtures; `main` has 7, so the tag is an older
snapshot. Consumers today clone the repository and pass `--fixtures-dir`
(`docs/fixture-contract.md` section 7). `docs/fixture-contract.md` section 8 and
`ROADMAP.md` list fetching remote versioned fixtures as out of scope for the MVP;
this contract is the deliberately scoped feature that section anticipates, and
it does not change the rule that the engine's loader reads only a local
directory.

Existing pieces to reuse, not duplicate:

- The engine release workflow already builds `stellar-canary-linux-x86_64` and a
  `SHA256SUMS` file on `main`; `v0.1.1` predates that and has no assets.
- The Action already parses checksum manifests and falls back to commit and tag
  pinning (`ProtocolCanary-Action/src/canary.ts`: `parseChecksumManifest`,
  `selectExpectedChecksum`, `resolveTagCommit`), with unit tests added through closed
  Action issues such as #259 to #261, #263 and #273 to #275.

## 2. Artifacts

For a pack release with version `V` (for example `1.0.0`):

| File | Purpose |
|---|---|
| `protocolcanary-fixtures-V.tar.gz` | The pack: fixture TOML files, payloads, and `registry.json` (CF-02). |
| `protocolcanary-fixtures-V.manifest.json` | Release manifest (below). Not inside the archive. |
| `protocolcanary-fixtures-V.sha256` | `sha256sum`-format line for the archive. |

Archive content is exactly the pack membership of CF-02 plus `registry.json` at
the root. No README, no `.git`, no symlinks, no extra files.

Reproducible build: entries sorted by path in UTF-8 byte order, owner uid/gid 0,
owner and group names empty, mtime fixed to `0`, mode `0644` for files and `0755`
for directories, no extended attributes, gzip with no embedded name or time
(`gzip -n`). Two builds from the same pack bytes must give the same archive
bytes. This is checked in CI by building twice.

## 3. Manifest (`manifestVersion` 1)

<!-- contract-example: release-manifest-v1 -->
```json
{
  "manifestVersion": 1,
  "name": "protocolcanary-fixtures",
  "version": "0.0.0",
  "protocols": [28],
  "revision": "0000000000000000000000000000000000000000",
  "packDigest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
  "registryVersion": 1,
  "archive": {
    "file": "protocolcanary-fixtures-0.0.0.tar.gz",
    "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
    "bytes": 0,
    "entries": 0,
    "uncompressedBytes": 0
  },
  "compatibleEngines": [
    { "version": "0.0.0", "verified": false }
  ]
}
```

Illustrative: none of these values is real.

- `revision` is the full git commit of the pack source. It is provenance only.
- `packDigest` is the CF-02 digest. `archive.sha256` hashes the compressed bytes.
- `compatibleEngines[].verified` is `true` only when that exact engine version
  and this exact pack were run together and the run is recorded under CF-04.
  An unverified combination may be listed with `false`. A manifest must never
  claim a range ("0.1.x"), because only exact combinations are verified.
- `protocols` lists the protocol numbers the pack contains fixtures for. A
  protocol with an empty pack (today `protocol-27`) is not listed.

## 4. Immutability

- A release tag, once published, is never moved or deleted. A mistake is fixed by
  publishing a new version and, if needed, a notice in the release text.
- A published artifact is never replaced. If the archive for `V` changes, `V` is
  burned and a new version is cut.
- The tag name format for new pack releases is `fixtures-vMAJOR.MINOR.PATCH`.
  The existing `protocol-28` tag keeps its meaning and is not re-pointed.
- Version rules: patch for fixes to data that do not change what a valid project
  needs to pass; minor for added fixtures; major for removed or semantically
  changed fixtures or a new `registryVersion`.

## 5. Verification order (consumers)

A consumer, in order, and stopping at the first failure:

1. Obtain the expected `archive.sha256` from a trusted source: the lockfile
   (CF-03) when pinned, otherwise the manifest from the same release over HTTPS.
2. Download over HTTPS only, following at most 3 redirects, to hosts on a short
   allow list (pending D-11: a release asset download redirects from
   `github.com` to a different GitHub-operated host, and the exact hosts must be
   taken from a real redirect and tested, not assumed). Enforce a maximum download size equal to
   the manifest's `archive.bytes` plus no slack, or a hard cap when no manifest
   exists.
3. Compare SHA-256 of the received bytes with the expected value.
4. Extract safely (section 6) into a new empty directory.
5. Recompute `packDigest` over the extracted tree and compare with the
   manifest, then with the lockfile if present.
6. Validate `registry.json` against CF-02.

If a manifest and a lockfile disagree, the lockfile wins and the run fails with
the mismatch shown; the manifest is never allowed to override a pin.

Checksums fetched from the same origin as the archive protect against corruption
and partial downloads, not against a compromised release. Authenticity beyond
that needs a pin held elsewhere (the lockfile in the consumer's repository) or a
signature (D-07: whether to adopt signed releases is a maintainer decision and
is not designed here).

## 6. Safe extraction

Reject the whole archive, extracting nothing further, when any entry:

- has an absolute path, a `..` segment, a backslash, a NUL, or a path outside the
  CF-02 path rules;
- is anything other than a regular file or a directory (symlink, hard link,
  device, fifo);
- duplicates another path, including after Unicode case folding;
- would make the entry count, total uncompressed bytes, single-file size or path
  depth exceed the limits. Proposed defaults, pending D-06: 2,000 entries,
  16 MiB uncompressed in total, 1 MiB per file, depth 8 (for scale, the Protocol 28 pack is 8 files and 17,102 bytes on 2026-10-09);
- sets setuid, setgid or sticky bits (modes are ignored, and files are created
  `0644`).

Extract into a fresh directory created by the consumer, with each entry opened
by `create_new` semantics so it cannot overwrite or follow a pre-existing link.
Never extract into the project tree. Never execute anything from the archive.

## 7. Compatibility

- Pack to engine: an engine supports specific `registryVersion` values and
  fixture-format versions. `docs/fixture-contract.md` is fixture format v1. An
  engine given a `registryVersion` it does not know exits `4` with that message.
- Pack to Action: the Action does not interpret the pack; it passes a verified
  directory (or digest and release) to the CLI.
- Engine release to Action default: unchanged rule from `docs/architecture.md`;
  the Action pins an exact engine version.
- The compatibility table in a release is generated from CF-04 records, not
  hand-edited.

## 8. Security and trust boundaries

Archives are untrusted until section 5 passes. The pack contains declarative data
only. No credentials are needed to fetch a public release; if a private repository
is used the token is passed in a header, never in a URL, and is not written to
logs, reports or artifacts. Nothing in this flow signs or submits a Stellar
transaction or handles a Stellar secret key.

## 9. Validation and test requirements

- Reproducible-build test (build twice, compare bytes).
- Negative archive corpus generated by the test, not downloaded: traversal,
  absolute path, symlink, hard link, duplicate, case-fold duplicate, oversize,
  too many entries, bad mode bits, truncated gzip, wrong checksum.
- A positive round trip: build, verify, extract, digest equal to CF-02 vectors.
- Redirect to a disallowed host is refused.

## 10. Cross-repository dependencies

CF-02 (digest, registry), CF-03 (pins), CF-04 (compatibility evidence). The
Action implements steps 1 to 3 and delegates 4 to 6 to a CLI subcommand so the
logic lives once, in the CLI. The CLI does not download anything unless a
maintainer decision adds it; until then the Action downloads and the CLI verifies
an extracted or archived local path.

## 11. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CF-06-A Deterministic archive builder script | ProtocolCanary-Fixtures | CF-02-B |
| CF-06-B Manifest generator and schema | ProtocolCanary-Fixtures | CF-06-A |
| CF-06-C Release workflow that builds twice, compares, then uploads | ProtocolCanary-Fixtures | CF-06-A, CF-06-B |
| CF-06-D Negative archive corpus generator | Protocol-Canary | this contract |
| CF-06-E Safe extractor library with limits | Protocol-Canary | CF-06-D |
| CF-06-F `fixture-pack verify` command (archive and directory, read only) | Protocol-Canary | CF-06-E, CF-02-F |
| CF-06-G Action: download with redirect and size limits | ProtocolCanary-Action | this contract |
| CF-06-H Action: `fixtures-pack` input and mutual exclusion with `fixtures-dir` | ProtocolCanary-Action | CF-06-F released |
| CF-06-I Action: digest-keyed cache for verified packs | ProtocolCanary-Action | CF-06-G |
