# CF-02: Fixture registry and pack digest

Status: frozen for implementation planning, contract version 1.
Pending maintainer decision: D-03 (where the registry lives).
Authoritative for: registry JSON structure, canonical ordering, hashing inputs
and the pack digest. Both the Rust engine and the Python tooling in
`ProtocolCanary-Fixtures` implement it and must agree byte for byte.

## 1. Purpose and existing behavior

There is no registry and no digest today. Facts checked on 2026-10-09:

- `canary_fixtures::load_directory` walks the given directory recursively
  (`collect_toml_files`), sorts paths, and parses **every** `*.toml` file as a
  fixture. It descends into hidden directories and follows symlinked
  directories (`Path::is_dir` follows links).
- A fixture may name `input_file` and `expected_file`. They are resolved with
  `dir.join(name)` with no confinement (an absolute path or `../` escapes the
  pack) and are only checked for existence by `validate`. No runner reads them
  yet.
- Fixture identity is the `id` string. It is unique across the loaded tree.
- `ProtocolCanary-Fixtures` validates fixtures with `tools/validate/validate.py`
  and `schemas/fixture-v1.schema.json`. It has one git tag, `protocol-28`, with
  no release assets and no checksum.

A pack digest gives "which exact fixtures ran" a stable answer that does not
depend on a path, a checkout tool or a commit hash.

## 2. Pack membership

A **pack** is a directory root. Its files are:

1. every regular file whose name ends in `.toml`, at any depth (this matches
   the loader, so the registry can never list a different set than the engine
   would load);
2. every regular file referenced by a fixture's `input_file` or `expected_file`.

Everything else (README, schemas, docs, `.git`) is outside the pack and does
not affect the digest.

Pack-relative paths must satisfy all of:

- valid UTF-8, using `/` as separator, no leading `/`, no `.` or `..` segment,
  no empty segment, no `\`, no NUL, no control characters;
- no two paths equal after Unicode case folding (prevents a pack that differs
  between case-sensitive and case-insensitive filesystems);
- every file is a regular file. A symlink anywhere inside the pack, to a file
  or a directory, makes the pack invalid.
- a referenced payload path must resolve inside the pack root after applying the
  rules above. A reference that escapes the root makes the pack invalid.

An invalid pack is rejected with exit code `4` by the engine and a non-zero exit
by the Python generator. It is never silently skipped.

## 3. Registry structure (`registryVersion` 1)

One registry per protocol pack. Illustrative, not real digests:

<!-- contract-example: registry-v1 -->
```json
{
  "registryVersion": 1,
  "protocol": 28,
  "fixtures": [
    {
      "id": "example-xdr-fixture",
      "path": "xdr/example-xdr-fixture.toml",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
      "surface": "xdr",
      "category": "example",
      "description": "illustrative entry",
      "sourceReference": "https://example.invalid/reference",
      "requiredCapabilities": [],
      "payloads": [
        { "path": "xdr/example-input.bin", "sha256": "0000000000000000000000000000000000000000000000000000000000000000" }
      ]
    }
  ],
  "packDigest": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
}
```

Required: `registryVersion`, `protocol`, `fixtures`, `packDigest`. Per fixture:
`id`, `path`, `sha256`, `surface`, `category`, `description`, `payloads` (may be
empty). Optional: `sourceReference` (mirrors the TOML, absent when the TOML omits
it), `requiredCapabilities` (absent equals empty). The registry copies metadata
for browsing; the TOML file stays the source of truth, and a validator must
fail when a registry value differs from the TOML value.

JSON encoding: UTF-8, LF line endings, two-space indent, one trailing newline,
object keys in the order shown above, no duplicate keys. Arrays are sorted as in
section 4. The registry is committed or generated in a way that makes it
byte-reproducible (D-03).

The registry excludes git commits, timestamps, absolute paths and host
information. A pack's git revision is provenance and is recorded elsewhere
(CF-03, CF-06), never inside the digest.

## 4. Canonical ordering

- `fixtures` sorted by `id`, comparing UTF-8 bytes.
- `payloads` sorted by `path`, comparing UTF-8 bytes.
- A payload shared by two fixtures appears once per fixture that references it
  and once in the digest input (see below).

## 5. Hashing

File hash: `sha256` over the raw file bytes. No newline normalization, no
trimming, no BOM handling. A checkout that rewrites line endings changes the
digest and that is correct: it is a different byte sequence. The Fixtures
repository should add a `.gitattributes` forcing `eol=lf` for pack files so the
digest does not depend on a contributor's `core.autocrlf`.

Pack digest input: the ASCII line `canary-pack-v1` followed by `\n`, then for
every distinct pack file sorted by pack-relative path (UTF-8 bytes), one line:

```text
<sha256 hex lowercase> <decimal byte length> <relative path>\n
```

`packDigest` is `sha256:` plus the lowercase hex SHA-256 of that text. Each file
appears exactly once, whether it is a fixture or a payload.

Properties this guarantees: independent of root path, of file system order, of
timestamps and permissions, and of whether the pack was read from a checkout or
an extracted archive. It changes when any byte of any fixture or payload
changes, when a file is added, removed or renamed.

**Not the same as the engine's cache digest.** The engine's result cache keeps
its own per-fixture content digest (the fixture file plus the payloads it
references, length-framed) so that an edited fixture cannot reuse a result. That
value is internal to the local cache, differs in construction from the file
hashes and the pack digest defined here, and is never written to a registry, a
lockfile or a report. Do not compare the two.

**Revision versus content digest.** `packDigest` identifies content. Two commits
with identical pack bytes have the same digest. A git revision identifies
history. Consumers pin the digest and may record the revision for humans.

## 6. Validation

A registry validator (Python, in `ProtocolCanary-Fixtures`) and a verifier
(Rust, in the engine) must both implement:

1. Pack path rules (section 2). Reject, do not repair.
2. Recompute every file hash and the pack digest, compare with the registry.
3. Registry ids equal the TOML ids; registry set equals the set the engine
   loader would load (`.toml` files); no fixture missing from the registry, no
   extra entry.
4. Registry metadata equals TOML metadata.
5. Registry bytes equal the canonical encoding (re-encode and compare).
6. Freshness: CI regenerates the registry and fails on any difference.

## 7. Security and trust boundaries

A digest proves "these bytes", not "these bytes are correct" and not "an
authorized party published them". Trust in a pack comes from where its digest
was obtained (a lockfile reviewed in the consumer's repo, a release the
maintainers published). The generator and verifier read files only; they never
execute anything. Both must cap file count and total size (defaults in CF-06)
and must refuse symlinks, so a hostile pack cannot make a verifier read outside
its root or loop.

## 8. Cross-repository dependencies

- Engine: digest verification library and `fixtures --json` output (CF-01
  `fixturePack`).
- Fixtures: generator, schema for the registry, CI freshness check.
- Action: reads `packDigest` from a lockfile or release manifest and compares;
  does not compute it (CLI authoritative).
- CF-03 and CF-06 depend on this contract. CF-04 attaches verification records
  to registry entries.

## 9. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CF-02-A Shared conformance vectors: a small directory tree plus expected file hashes and pack digest, as test data | ProtocolCanary-Fixtures | this contract |
| CF-02-B Python generator for the registry | ProtocolCanary-Fixtures | CF-02-A |
| CF-02-C JSON Schema for registry v1 | ProtocolCanary-Fixtures | this contract |
| CF-02-D Python validator rules 1 to 5 | ProtocolCanary-Fixtures | CF-02-B |
| CF-02-E CI freshness check | ProtocolCanary-Fixtures | CF-02-B |
| CF-02-F Rust pack walker and digest (`canary-fixtures`) matching CF-02-A | Protocol-Canary | CF-02-A |
| CF-02-G Reject symlinks and escaping payload paths in the loader | Protocol-Canary | this contract |
| CF-02-H `.gitattributes` for LF pack files | ProtocolCanary-Fixtures | none |
