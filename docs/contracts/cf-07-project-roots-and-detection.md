# CF-07: Project roots and capability detection

Status: frozen for implementation planning, contract version 1.
Pending maintainer decision: D-08 (root discovery
rule when `--project-root` is absent). D-06 is approved (scan limit values).
Authoritative for: project root selection, supported manifests, dependency
identifiers, scan boundaries and the evidence behind detected capabilities.

## 1. Purpose and existing behavior

`canary_project::detect(root)` (`crates/canary-project/src/detector.rs`) is
verified, as of `main` at `1ff7908`, to do the following:

- The root is the process's current directory. There is no `--project-root` and no
  upward search.
- It reads exactly two manifests in the root: `Cargo.toml` (sections
  `dependencies`, `dev-dependencies`, `build-dependencies`, and the same three
  under `[workspace]`) and `package.json` (`dependencies`, `devDependencies`,
  `peerDependencies`). A manifest that is missing or fails to parse yields no
  signal and no diagnostic.
- It does not read `[workspace] members`, npm `workspaces`, or any nested
  manifest. A repository whose root is a virtual Cargo workspace therefore
  detects as `Unknown` even if members depend on `soroban-sdk`.
- It checks for `stellar.toml` in the root and scans for a `.wasm` file up to two
  directories deep, skipping `.git`, `node_modules` and `target` (known build
  output directories are probed directly).
- Dependency identifiers are matched as exact, case-sensitive strings
  (`ProjectManifest::has_dependency`), with no version information.
- Capability sets (`capabilities.rs`): Soroban `soroban-sdk`; Stellar SDK
  `stellar-sdk`, `stellar-xdr`, `stellar-base`, `stellar-strkey`,
  `@stellar/stellar-sdk`, `@stellar/stellar-base`; RPC client `stellar-rpc-client`,
  `soroban-rpc`, `@stellar/stellar-sdk`, `soroban-client`.
- Classification precedence: Soroban, then RPC consumer, then Stellar SDK, then
  generic (stellar.toml or wasm), else Unknown. An explicit `[project].type`
  always wins (`resolve_project_type`) and the report shows only the final type.
- `ProjectContext.root` is an absolute path. The report exposes only the
  directory name as `project.name`.

## 2. Root selection

Rules, in order:

1. `--project-root <path>` (planned): the directory, canonicalized once. It must
   exist and be a directory. All relative inputs (`--config`, `--fixtures-dir`,
   `--output`, lock path) resolve against it, not against the caller's working
   directory, and the choice is shown in `inspect`.
2. Otherwise the current directory, exactly as today.

Upward discovery (finding a repository root from a subdirectory) is not part of
version 1 (D-08 asks whether to add it). The Git context is read from the
selected root, and `git.commit` is the commit of the repository containing it;
a root inside a nested git worktree uses that worktree's HEAD.

A monorepo member is selected by pointing `--project-root` at it. The engine never
guesses which member to check, and never merges the capabilities of several
members into one project.

## 3. Supported manifest formats

Tier 1, implemented: `Cargo.toml`, `package.json` at the root.

Tier 2, specified here, implementation work:

| Ecosystem | File | Declared dependency source | Notes |
|---|---|---|---|
| Rust workspace | `Cargo.toml` `[workspace] members` and `exclude` | member `Cargo.toml` files | Globs `*` and `?` only. A virtual workspace root with no `[package]` is not a project by itself. |
| npm workspaces | `package.json` `workspaces` (array or `{ packages }`) | member `package.json` | Local paths only. |
| pnpm | `pnpm-workspace.yaml` `packages` | member `package.json` | A minimal YAML subset: a top-level `packages` list of strings with optional `!` negation. Anything else is reported unsupported. |
| Go | `go.mod` | `require` directives, single line and block | `replace` and `exclude` are not followed. No `go` command is run. |
| Python | `pyproject.toml` | PEP 621 `project.dependencies` and `project.optional-dependencies`; `[tool.poetry.dependencies]` | Other build backends: unsupported. |
| Python | `requirements*.txt` at the root | one requirement per line | `-r`/`-c` includes, URLs, `${}` substitutions: reported, not followed. |
| Java | `pom.xml` | `<dependency>` `groupId`, `artifactId` | XML parsed with DTDs and external entities disabled. |
| Gradle | `build.gradle`, `build.gradle.kts` | only literal `group:name:version` strings in dependency configurations | Anything computed (variables, version catalogs, `project(...)`) is `unknown`. |

No tier runs a package manager, a build tool, a compiler or a script.

## 4. Verified dependency identifiers

An identifier enters the detection vocabulary only when it is verified to exist
for that ecosystem and to be a Stellar library. Checked on 2026-10-09:

| Ecosystem | Identifier | Evidence | Status |
|---|---|---|---|
| crates.io | `soroban-sdk`, `stellar-xdr`, `stellar-strkey`, `stellar-rpc-client`, `soroban-client`, `soroban-rpc`, `stellar-base`, `stellar-sdk` | crates.io API lookups returned each crate | Existing; `soroban-rpc` last updated 2024-02-22 and `stellar-sdk` 2023-11-01, which are legacy. |
| crates.io | `rs-stellar-rpc-client` | The Stellar docs client-SDK page names this crate, but crates.io has no crate of that name; the real name is `stellar-rpc-client` | **Do not add.** Docs discrepancy only. |
| npm | `@stellar/stellar-sdk`, `@stellar/stellar-base`, `stellar-sdk`, `soroban-client` | npm registry returned each | Existing. `stellar-sdk` and `soroban-client` are legacy names. |
| PyPI | `stellar-sdk` | PyPI JSON API; project home is `StellarCN/py-stellar-base`, which the docs page lists | Verified, not yet detected. |
| Go | `github.com/stellar/go-stellar-sdk` | Go module proxy `go.mod` declares this module path; Stellar docs list the repository | Verified, not yet detected. |
| Go | `github.com/stellar/go` | Module proxy returns a version dated 2025-12-10; this is the previous path of the same SDK | Verified to exist; confirm with maintainers whether to include. |
| Maven | none confirmed | The Stellar docs client-SDK page lists the Java SDK repository (`lightsail-network/java-stellar-sdk`) but gives no Maven coordinates, and one Maven Central search for a guessed coordinate returned 0 results (that search is not proof either way) | **Unverified. Blocked.** Do not implement Maven or Gradle detection for Stellar coordinates until coordinates are confirmed from Maven Central or the SDK's own documentation. |

Parsing support for Maven and Gradle may be built behind an empty vocabulary, so
the parser work is not blocked, but it must not ship with guessed coordinates.

## 5. Scan boundaries

- Never follow symlinks (files or directories). Report each skipped link.
- Never leave the selected root.
- Excluded directory names, anywhere: `.git`, `node_modules`, `target`, `vendor`,
  `dist`, `build`, `.venv`, `venv`, `__pycache__`, `.gradle`, `.cache`.
- Bounds (approved under D-06): depth 4 below the root, 2,000 directories
  visited, 10,000 directory entries read, 1 MiB per manifest read, 64 workspace
  members. Hitting any bound stops that part of the scan and records a
  diagnostic; it never turns into a pass or into a silent `Unknown`.
- Errors reading one file are diagnostics for that file and do not abort the
  scan. Unparseable manifests are listed as `unparseable`.
- Deterministic: directory entries visited in UTF-8 byte order of the
  root-relative path.

## 6. Capability evidence

Detection returns, for every capability, the evidence that produced it.
Paths are root-relative with `/` separators; absolute paths never appear.
Illustrative shape:

<!-- contract-example: capability-evidence-v1 -->
```json
{
  "evidenceVersion": 1,
  "root": ".",
  "capabilities": [
    {
      "capability": "soroban-contract",
      "evidence": [
        { "manifest": "contracts/token/Cargo.toml", "ecosystem": "cargo", "dependency": "soroban-sdk", "section": "dependencies" }
      ]
    }
  ],
  "inspected": ["Cargo.toml", "contracts/token/Cargo.toml"],
  "diagnostics": [
    { "code": "scan-bound-reached", "detail": "directories visited limit 2000" }
  ],
  "projectType": { "value": "soroban", "source": "detected" }
}
```

`projectType.source` is `detected` or `config`. When config overrides a
different detected type, both are shown (`detectedValue`). Nothing is silently
discarded.

Diagnostic codes: `manifest-unparseable`, `manifest-too-large`,
`scan-bound-reached`, `symlink-skipped`, `unsupported-syntax`,
`workspace-member-missing`, `path-outside-root`.

## 7. Security and trust boundaries

The project directory is untrusted input (a pull request controls it in CI).
Parsers must be pure: no process execution, no network, no environment
expansion, no include following outside the root, XML external entities off, size
and depth caps, no recursion on user-controlled structure without a limit. A
manifest cannot cause a file outside the root to be read. Evidence is safe to
put in a shared report because it contains no absolute paths and no file
contents beyond dependency names.

## 8. Validation and test requirements

- Each tier 2 parser: accept, reject, unsupported-syntax and size-limit tests
  from files the test creates.
- A virtual Cargo workspace whose members use `soroban-sdk` must be detected
  with member evidence once workspace support lands, and must stay `Unknown`
  without it (regression guard for the current behavior).
- Symlink loop and symlink escape tests.
- A generated 100,000 file tree must finish within the bounds without
  exhausting memory.
- Evidence output stable across two runs and two operating systems.

## 9. Cross-repository dependencies

The Action's `working-directory` input (CF-08) maps to `--project-root`. CF-01
`project` stays name and type; evidence output is a separate command so report
v1 does not grow. Fixtures' `required_capabilities` enum must match the engine's
`Capability` set (`ProtocolCanary-Fixtures/tools/validate/validate.py:
CAPABILITIES`), checked by a cross-language test.

## 10. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CF-07-A `--project-root` for check, inspect, fixtures | Protocol-Canary | this contract |
| CF-07-B Evidence model and `inspect --format json` | Protocol-Canary | this contract |
| CF-07-C Cargo workspace members | Protocol-Canary | CF-07-B |
| CF-07-D npm workspaces | Protocol-Canary | CF-07-B |
| CF-07-E pnpm workspace file | Protocol-Canary | CF-07-D |
| CF-07-F `go.mod` parser and vocabulary | Protocol-Canary | CF-07-B |
| CF-07-G `pyproject.toml` parser | Protocol-Canary | CF-07-B |
| CF-07-H `requirements.txt` parser | Protocol-Canary | CF-07-B |
| CF-07-I Scan bounds and diagnostics | Protocol-Canary | CF-07-B |
| CF-07-J `pom.xml` parser (empty vocabulary until coordinates verified) | Protocol-Canary | CF-07-B |
| CF-07-K Gradle literal-only parser (same condition) | Protocol-Canary | CF-07-B |
| CF-07-L Enum parity test, Rust and Python | ProtocolCanary-Fixtures | none |
