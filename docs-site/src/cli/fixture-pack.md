# `fixture-pack`

Provides offline operations on fixture packs. Its primary subcommand,
`fixture-pack verify`, validates that a fixture pack directory matches
its `registry-v1.json` manifest by recomputing file checksums and the
canonical pack digest (CF-02).

```text
Verify a fixture pack against its registry manifest

Usage: stellar-canary fixture-pack <COMMAND>

Commands:
  verify  Verify a fixture pack directory against its registry-v1.json manifest. Computes the CF-02 canonical pack digest and verifies file checksums. Exits 0 on success, 6 on checksum mismatch or verification failure
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `fixture-pack verify`

```text
Verify a fixture pack directory against its registry-v1.json manifest. Computes the CF-02 canonical pack digest and verifies file checksums. Exits 0 on success, 6 on checksum mismatch or verification failure

Usage: stellar-canary fixture-pack verify [OPTIONS] <PATH>

Arguments:
  <PATH>  Path to a fixture pack directory to verify

Options:
      --registry <PATH>  Path to the registry-v1.json manifest. If omitted, looks for registry.json in the pack root
  -h, --help             Print help
```

### Arguments

| Argument | Description |
|---|---|
| `<PATH>` | Required positional argument naming the fixture pack directory to verify. Must exist and be a directory. |
| `--registry <PATH>` | Optional path to the registry manifest (`registry-v1.json` schema). When omitted, defaults to `registry.json` inside the root of the specified pack directory (`<PATH>/registry.json`). |

## Verification behavior

`stellar-canary fixture-pack verify` operates completely offline: it
performs no network requests and executes no test fixtures. It validates
the pack directory against the registry manifest through the following
stages:

1. **Path and directory validation**: Confirms that `<PATH>` exists and is a
   directory.
2. **Registry manifest loading**: Loads the specified or default
   `registry.json` file, parses it strictly against the `registryVersion: 1`
   schema (CF-02), and requires `registryVersion` to equal `1`.
3. **Canonical pack digest calculation**:
   - Walks the fixture pack directory, excluding `.git` directories and
     rejecting any symbolic link, junction, or invalid path (CF-02).
   - Collects all regular `*.toml` fixture files and all payload files
     referenced by `input_file` or `expected_file`.
   - Computes the SHA-256 hash and byte length of each distinct pack file.
   - Formats lines of `<sha256> <size> <relative_path>\n` sorted by
     pack-relative path in UTF-8 byte order, prefixed by `canary-pack-v1\n`.
   - Computes the SHA-256 digest of this canonical representation (yielding
     `sha256:<hex>`).
4. **Pack digest check**: Verifies that the computed pack digest matches the
   `packDigest` recorded in the registry manifest.
5. **Fixture and payload checksum checks**:
   - For every fixture listed in the manifest, verifies that the file exists
     and its SHA-256 hash matches the recorded `sha256`.
   - For every payload entry associated with a fixture, verifies that the
     payload file exists and its SHA-256 hash matches the recorded `sha256`.
6. **Completeness check**: Confirms that every file discovered during the
   pack walk is accounted for in the registry manifest (no unrecorded files
   are allowed in the pack digest).

## Exit outcomes

`fixture-pack verify` strictly distinguishes between an unusable or
malformed invocation (exit `2`) and a verification failure on an evaluated
pack (exit `6`):

| Exit code | Name | Meaning |
|---|---|---|
| `0` | `Pass` | Verification succeeded: all fixture and payload file checksums match the manifest, and the computed pack digest matches `packDigest`. |
| `2` | `ConfigurationError` | Unusable invocation or invalid pack path: `<PATH>` does not exist or is not a directory; the registry file cannot be read or is invalid JSON; `registryVersion` is not `1`; or the pack contains structural defects (e.g. symlinks, path traversal, or unparseable TOML). |
| `6` | `VerificationFailure` | Verification failed: the computed pack digest does not match the registry `packDigest`, a fixture or payload file is missing or has a checksum mismatch, or unrecorded files were found in the pack. |

### Success output (Exit `0`)

When verification passes, summary details are printed to standard output:

```text
Fixture pack verification passed:
  Protocol: 28
  Registry version: 1
  Pack digest: sha256:0123456789abcdef...
  Fixtures verified: 5
  All file checksums match registry.
  Pack digest matches registry.
```

### Verification failure output (Exit `6`)

When an evaluated pack does not match the registry, specific failure
diagnostics are printed to standard error:

```text
error: pack digest mismatch
  computed: 0123456789abcdef...
  expected: fedcba9876543210...
```

Or for a file checksum mismatch:

```text
error: fixture checksum mismatch for xdr/example.toml
  computed: 0123456789abcdef...
  expected: fedcba9876543210...
```

## Examples

Verify a fixture pack directory containing `registry.json` at its root:

```bash
stellar-canary fixture-pack verify ./fixtures
```

Verify a fixture pack against an external or generated registry manifest:

```bash
stellar-canary fixture-pack verify ./protocol-28 --registry ./generated-registry.json
```
