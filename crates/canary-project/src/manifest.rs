//! Lightweight manifest inspection.
//!
//! This intentionally does not use `cargo_metadata` or invoke package managers: for
//! detection purposes we only need the declared dependency names, and
//! reading manifests directly keeps detection fast and independent of
//! whether the target project's dependencies are even fetched yet.

use std::path::Path;

/// The dependency names declared by a project manifest (e.g. Cargo.toml, package.json).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectManifest {
    pub dependency_names: Vec<String>,
}

impl ProjectManifest {
    /// Returns `true` if the manifest declares a dependency with exactly
    /// this name.
    ///
    /// The comparison is a plain, case-sensitive whole-string match against
    /// [`ProjectManifest::dependency_names`]: the name must match in full,
    /// including any scope or registry prefix (e.g. `@stellar/stellar-sdk`,
    /// `soroban-sdk`); partial or fuzzy matches do not count. Names carry
    /// no version information, so this answers "is this dependency declared
    /// at all", not "is a compatible version declared".
    ///
    /// This never fails: an empty manifest simply returns `false`, and no
    /// error or panic condition exists.
    pub fn has_dependency(&self, name: &str) -> bool {
        self.dependency_names.iter().any(|d| d == name)
    }

    /// Returns `true` if the manifest declares at least one of `names`.
    ///
    /// A convenience wrapper over [`ProjectManifest::has_dependency`] for
    /// probing a group of alternative dependency names that all signal the same
    /// capability, as [`crate::capabilities::detect_capabilities`] does for the
    /// Soroban, Stellar SDK, and RPC client dependency sets. Matching stops at
    /// the first hit, so the order of `names` does not affect the result.
    ///
    /// An empty `names` slice matches nothing and returns `false`. Names are
    /// compared exactly, with no case folding, version, or scope normalization,
    /// so npm-scoped packages must be passed in full (for example
    /// `"@stellar/stellar-sdk"`). This method does not panic.
    ///
    /// # Examples
    ///
    /// ```
    /// use canary_project::ProjectManifest;
    ///
    /// let manifest = ProjectManifest {
    ///     dependency_names: vec!["stellar-rpc-client".to_string()],
    /// };
    ///
    /// // One match among several is enough.
    /// assert!(manifest.has_any_dependency(&["stellar-sdk", "stellar-rpc-client"]));
    ///
    /// // No match, and an empty candidate list, are both `false`.
    /// assert!(!manifest.has_any_dependency(&["soroban-sdk"]));
    /// assert!(!manifest.has_any_dependency(&[]));
    /// ```
    pub fn has_any_dependency(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.has_dependency(n))
    }
}

/// Reads and parses `<root>/Cargo.toml`, if present.
///
/// Returns `None` (not an error) when there is no `Cargo.toml` or it fails
/// to parse: a missing/unreadable manifest is a detection signal, not a
/// hard failure.
pub fn read_cargo_manifest(root: &Path) -> Option<ProjectManifest> {
    let raw = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    parse_cargo_manifest(&raw)
}

/// Parses `Cargo.toml` text into the dependency names it declares, or
/// `None` when it is not valid TOML. Shared by the root and workspace-member
/// readers so both see the same sections.
pub(crate) fn parse_cargo_manifest(raw: &str) -> Option<ProjectManifest> {
    let value: toml::Value = toml::from_str(raw).ok()?;

    let mut dependency_names = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = value.get(section).and_then(toml::Value::as_table) {
            dependency_names.extend(table.keys().cloned());
        }
        if let Some(table) = value
            .get("workspace")
            .and_then(|w| w.get(section))
            .and_then(toml::Value::as_table)
        {
            dependency_names.extend(table.keys().cloned());
        }
    }
    dependency_names.sort();
    dependency_names.dedup();

    Some(ProjectManifest { dependency_names })
}

/// One declared dependency along with its manifest section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestDep {
    pub name: String,
    pub section: String,
}

/// Parses `Cargo.toml` text into declared dependencies with their section names.
pub fn parse_cargo_dependencies(raw: &str) -> Option<Vec<ManifestDep>> {
    let value: toml::Value = toml::from_str(raw).ok()?;

    let mut deps = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = value.get(section).and_then(toml::Value::as_table) {
            for key in table.keys() {
                deps.push(ManifestDep {
                    name: key.clone(),
                    section: section.to_string(),
                });
            }
        }
        if let Some(table) = value
            .get("workspace")
            .and_then(|w| w.get(section))
            .and_then(toml::Value::as_table)
        {
            for key in table.keys() {
                deps.push(ManifestDep {
                    name: key.clone(),
                    section: format!("workspace.{section}"),
                });
            }
        }
    }

    Some(deps)
}

/// Reads and parses `<root>/package.json`, if present.
pub fn read_package_json(root: &Path) -> Option<ProjectManifest> {
    let raw = std::fs::read_to_string(root.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;

    let mut dependency_names = Vec::new();
    for section in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(obj) = value.get(section).and_then(|v| v.as_object()) {
            dependency_names.extend(obj.keys().cloned());
        }
    }
    dependency_names.sort();
    dependency_names.dedup();

    Some(ProjectManifest { dependency_names })
}

/// Parses `package.json` text into declared dependencies with their section names.
pub fn parse_package_json_dependencies(raw: &str) -> Option<Vec<ManifestDep>> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;

    let mut deps = Vec::new();
    for section in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(obj) = value.get(section).and_then(|v| v.as_object()) {
            for key in obj.keys() {
                deps.push(ManifestDep {
                    name: key.clone(),
                    section: section.to_string(),
                });
            }
        }
    }

    Some(deps)
}

/// Parses runtime and development dependencies from a workspace member manifest.
pub(crate) fn parse_workspace_package_json_dependencies(raw: &str) -> Option<Vec<ManifestDep>> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let mut deps = Vec::new();
    for section in ["dependencies", "devDependencies"] {
        if let Some(obj) = value.get(section).and_then(|value| value.as_object()) {
            for key in obj.keys() {
                deps.push(ManifestDep {
                    name: key.clone(),
                    section: section.to_string(),
                });
            }
        }
    }
    Some(deps)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PnpmWorkspaceParseError {
    InvalidYaml,
    Unsupported,
}

/// Parses the contract's minimal pnpm YAML subset: one top-level packages list
/// containing only string patterns.
pub(crate) fn parse_pnpm_workspace_patterns(
    raw: &str,
) -> Result<Vec<String>, PnpmWorkspaceParseError> {
    let value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(raw).map_err(|_| PnpmWorkspaceParseError::InvalidYaml)?;
    let Some(mapping) = value.as_mapping() else {
        return Err(PnpmWorkspaceParseError::Unsupported);
    };
    if mapping.len() != 1 || !mapping.contains_key(serde_yaml_ng::Value::String("packages".into()))
    {
        return Err(PnpmWorkspaceParseError::Unsupported);
    }
    let Some(packages) = value
        .get("packages")
        .and_then(serde_yaml_ng::Value::as_sequence)
    else {
        return Err(PnpmWorkspaceParseError::Unsupported);
    };
    packages
        .iter()
        .map(|pattern| {
            pattern
                .as_str()
                .map(str::to_string)
                .ok_or(PnpmWorkspaceParseError::Unsupported)
        })
        .collect()
}

/// An error encountered while parsing a `go.mod` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoModError {
    /// The 1-based line number where the error occurred.
    pub line: usize,
    /// Description of the parsing error.
    pub message: String,
}

impl std::fmt::Display for GoModError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for GoModError {}

fn tokenize_line(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            break;
        }
        if c == '(' {
            tokens.push("(".to_string());
            i += 1;
            continue;
        }
        if c == ')' {
            tokens.push(")".to_string());
            i += 1;
            continue;
        }
        if c == '=' && i + 1 < chars.len() && chars[i + 1] == '>' {
            tokens.push("=>".to_string());
            i += 2;
            continue;
        }
        if c == '"' {
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() {
                    let next = chars[i + 1];
                    match next {
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        other => {
                            s.push('\\');
                            s.push(other);
                        }
                    }
                    i += 2;
                } else if ch == '"' {
                    closed = true;
                    i += 1;
                    break;
                } else {
                    s.push(ch);
                    i += 1;
                }
            }
            if !closed {
                return Err("unclosed double quote".to_string());
            }
            tokens.push(s);
            continue;
        }
        if c == '`' {
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '`' {
                    closed = true;
                    i += 1;
                    break;
                } else {
                    s.push(ch);
                    i += 1;
                }
            }
            if !closed {
                return Err("unclosed raw string literal".to_string());
            }
            tokens.push(s);
            continue;
        }
        let mut s = String::new();
        while i < chars.len() {
            let ch = chars[i];
            if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '"' || ch == '`' {
                break;
            }
            if ch == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
                break;
            }
            if ch == '=' && i + 1 < chars.len() && chars[i + 1] == '>' {
                break;
            }
            s.push(ch);
            i += 1;
        }
        tokens.push(s);
    }
    Ok(tokens)
}

#[derive(Debug, PartialEq, Eq)]
enum BlockState {
    None,
    Require,
    Other(String),
}

const TOP_LEVEL_DIRECTIVES: &[&str] = &[
    "module",
    "go",
    "toolchain",
    "require",
    "replace",
    "exclude",
    "retract",
    "use",
    "godebug",
];

const BLOCK_DIRECTIVES: &[&str] = &["require", "replace", "exclude", "retract", "use", "godebug"];

/// Parses `go.mod` text into declared dependencies with their section names.
///
/// Supports single-line and block `require` directives without executing external
/// commands. Does not follow `replace` or `exclude` directives. Returns an error if
/// the manifest has malformed syntax.
pub fn parse_go_mod_dependencies(raw: &str) -> Result<Vec<ManifestDep>, GoModError> {
    let clean = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut deps = Vec::new();
    let mut state = BlockState::None;

    for (line_idx, line) in clean.lines().enumerate() {
        let line_no = line_idx + 1;
        let tokens = tokenize_line(line).map_err(|msg| GoModError {
            line: line_no,
            message: msg,
        })?;

        if tokens.is_empty() {
            continue;
        }

        match &state {
            BlockState::None => {
                if let Some(open_pos) = tokens.iter().position(|t| t == "(") {
                    if open_pos != 1 {
                        return Err(GoModError {
                            line: line_no,
                            message: format!("unexpected '(' in line: {line}"),
                        });
                    }
                    let directive = &tokens[0];
                    if !BLOCK_DIRECTIVES.contains(&directive.as_str()) {
                        return Err(GoModError {
                            line: line_no,
                            message: format!(
                                "directive '{directive}' does not support block syntax"
                            ),
                        });
                    }

                    if let Some(close_pos) = tokens.iter().position(|t| t == ")") {
                        if close_pos < open_pos {
                            return Err(GoModError {
                                line: line_no,
                                message: "unexpected ')' before '('".to_string(),
                            });
                        }
                        if close_pos != tokens.len() - 1 {
                            return Err(GoModError {
                                line: line_no,
                                message: "unexpected tokens after ')'".to_string(),
                            });
                        }
                        if directive == "require" {
                            let inner = &tokens[open_pos + 1..close_pos];
                            if inner.is_empty() {
                                // Empty require (), valid
                            } else if inner.len() == 2 {
                                deps.push(ManifestDep {
                                    name: inner[0].clone(),
                                    section: "require".to_string(),
                                });
                            } else {
                                return Err(GoModError {
                                    line: line_no,
                                    message: format!(
                                        "require block entry expects module path and version, found {inner:?}"
                                    ),
                                });
                            }
                        }
                    } else {
                        if tokens.len() != 2 {
                            return Err(GoModError {
                                line: line_no,
                                message: format!(
                                    "unexpected tokens after '(' on directive line: {line}"
                                ),
                            });
                        }
                        if directive == "require" {
                            state = BlockState::Require;
                        } else {
                            state = BlockState::Other(directive.clone());
                        }
                    }
                } else if tokens.contains(&")".to_string()) {
                    return Err(GoModError {
                        line: line_no,
                        message: "unexpected closing parenthesis ')'".to_string(),
                    });
                } else {
                    let directive = &tokens[0];
                    if !TOP_LEVEL_DIRECTIVES.contains(&directive.as_str()) {
                        return Err(GoModError {
                            line: line_no,
                            message: format!("unknown directive '{directive}'"),
                        });
                    }

                    if directive == "require" {
                        if tokens.len() != 3 {
                            return Err(GoModError {
                                line: line_no,
                                message: "require directive expects module path and version"
                                    .to_string(),
                            });
                        }
                        deps.push(ManifestDep {
                            name: tokens[1].clone(),
                            section: "require".to_string(),
                        });
                    } else if tokens.len() < 2 {
                        return Err(GoModError {
                            line: line_no,
                            message: format!("directive '{directive}' expects arguments"),
                        });
                    }
                }
            }
            BlockState::Require => {
                if tokens.contains(&"(".to_string()) {
                    return Err(GoModError {
                        line: line_no,
                        message: "nested block is not permitted in go.mod".to_string(),
                    });
                }
                if let Some(close_pos) = tokens.iter().position(|t| t == ")") {
                    if close_pos == 0 {
                        if tokens.len() > 1 {
                            return Err(GoModError {
                                line: line_no,
                                message: "unexpected tokens after ')'".to_string(),
                            });
                        }
                        state = BlockState::None;
                    } else if close_pos == tokens.len() - 1 {
                        let inner = &tokens[..close_pos];
                        if inner.len() == 2 {
                            if TOP_LEVEL_DIRECTIVES.contains(&inner[0].as_str()) {
                                return Err(GoModError {
                                    line: line_no,
                                    message: format!(
                                        "directive '{}' inside require block",
                                        inner[0]
                                    ),
                                });
                            }
                            deps.push(ManifestDep {
                                name: inner[0].clone(),
                                section: "require".to_string(),
                            });
                            state = BlockState::None;
                        } else {
                            return Err(GoModError {
                                line: line_no,
                                message: format!(
                                    "require block entry expects module path and version, found {inner:?}"
                                ),
                            });
                        }
                    } else {
                        return Err(GoModError {
                            line: line_no,
                            message: "unexpected tokens after ')'".to_string(),
                        });
                    }
                } else {
                    if TOP_LEVEL_DIRECTIVES.contains(&tokens[0].as_str()) {
                        return Err(GoModError {
                            line: line_no,
                            message: format!("directive '{}' inside require block", tokens[0]),
                        });
                    }
                    if tokens.len() != 2 {
                        return Err(GoModError {
                            line: line_no,
                            message: format!(
                                "require block entry expects module path and version, found {tokens:?}"
                            ),
                        });
                    }
                    deps.push(ManifestDep {
                        name: tokens[0].clone(),
                        section: "require".to_string(),
                    });
                }
            }
            BlockState::Other(_) => {
                if tokens.contains(&"(".to_string()) {
                    return Err(GoModError {
                        line: line_no,
                        message: "nested block is not permitted in go.mod".to_string(),
                    });
                }
                if let Some(close_pos) = tokens.iter().position(|t| t == ")") {
                    if close_pos != 0 && close_pos != tokens.len() - 1 {
                        return Err(GoModError {
                            line: line_no,
                            message: "unexpected tokens around ')'".to_string(),
                        });
                    }
                    state = BlockState::None;
                }
            }
        }
    }

    match state {
        BlockState::None => Ok(deps),
        BlockState::Require => Err(GoModError {
            line: clean.lines().count().max(1),
            message: "unclosed require block".to_string(),
        }),
        BlockState::Other(d) => Err(GoModError {
            line: clean.lines().count().max(1),
            message: format!("unclosed {d} block"),
        }),
    }
}

/// Parses `go.mod` text into the dependency names it declares, or
/// an error when it has malformed syntax.
pub fn parse_go_mod(raw: &str) -> Result<ProjectManifest, GoModError> {
    let deps = parse_go_mod_dependencies(raw)?;
    let mut dependency_names: Vec<String> = deps.into_iter().map(|d| d.name).collect();
    dependency_names.sort();
    dependency_names.dedup();
    Ok(ProjectManifest { dependency_names })
}

/// Reads and parses `<root>/go.mod`, if present.
///
/// Returns `None` when there is no `go.mod` or it fails to parse.
pub fn read_go_mod(root: &Path) -> Option<ProjectManifest> {
    let raw = std::fs::read_to_string(root.join("go.mod")).ok()?;
    parse_go_mod(&raw).ok()
}

/// Parses `go.mod` text into the dependency names it declares, or
/// `None` when it is not valid syntax.
pub fn parse_go_manifest(raw: &str) -> Option<ProjectManifest> {
    parse_go_mod(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, contents: &str) {
        std::fs::write(dir.join("Cargo.toml"), contents).unwrap();
    }

    fn write_package_json(dir: &Path, contents: &str) {
        std::fs::write(dir.join("package.json"), contents).unwrap();
    }

    #[test]
    fn reads_direct_and_workspace_dependencies() {
        let dir = super::super::test_support::temp_dir("manifest-direct");
        write_manifest(
            &dir.path,
            r#"
            [package]
            name = "example"
            version = "0.1.0"

            [dependencies]
            soroban-sdk = "22"

            [workspace.dependencies]
            stellar-xdr = "28"
            "#,
        );

        let manifest = read_cargo_manifest(&dir.path).expect("manifest");
        assert!(manifest.has_dependency("soroban-sdk"));
        assert!(manifest.has_dependency("stellar-xdr"));
        assert!(!manifest.has_dependency("nonexistent"));
    }

    #[test]
    fn reads_package_json_dependencies() {
        let dir = super::super::test_support::temp_dir("manifest-package");
        write_package_json(
            &dir.path,
            r#"{
                "name": "example",
                "dependencies": {
                    "@stellar/stellar-sdk": "^12.0.0"
                },
                "devDependencies": {
                    "typescript": "^5.0.0"
                }
            }"#,
        );

        let manifest = read_package_json(&dir.path).expect("manifest");
        assert!(manifest.has_dependency("@stellar/stellar-sdk"));
        assert!(manifest.has_dependency("typescript"));
        assert!(!manifest.has_dependency("nonexistent"));
    }

    #[test]
    fn manifest_without_a_dependencies_table_parses_with_no_dependencies() {
        let dir = super::super::test_support::temp_dir("manifest-no-dependencies");
        write_manifest(
            &dir.path,
            r#"
            [package]
            name = "dependency-free"
            version = "0.1.0"
            "#,
        );

        let manifest =
            read_cargo_manifest(&dir.path).expect("a valid [package]-only manifest parses");
        assert!(manifest.dependency_names.is_empty());
        assert!(!manifest.has_dependency("soroban-sdk"));
        assert!(!manifest.has_any_dependency(&["soroban-sdk", "stellar-rpc-client"]));
    }

    #[test]
    fn missing_manifest_returns_none() {
        let dir = super::super::test_support::temp_dir("manifest-missing");
        assert!(read_cargo_manifest(&dir.path).is_none());
        assert!(read_package_json(&dir.path).is_none());
        assert!(read_go_mod(&dir.path).is_none());
    }

    #[test]
    fn has_any_dependency_matches_if_one_name_is_present() {
        let manifest = ProjectManifest {
            dependency_names: vec!["stellar-rpc-client".to_string()],
        };
        assert!(manifest.has_any_dependency(&["stellar-sdk", "stellar-rpc-client"]));
        assert!(!manifest.has_any_dependency(&["soroban-sdk"]));
    }

    #[test]
    fn reads_go_mod_single_line_require() {
        let raw = "module example.com/my/app\n\ngo 1.22\n\nrequire github.com/stellar/go-stellar-sdk v1.0.0\n";
        let manifest = parse_go_mod(raw).expect("valid go.mod");
        assert_eq!(
            manifest.dependency_names,
            vec!["github.com/stellar/go-stellar-sdk"]
        );
        assert!(manifest.has_dependency("github.com/stellar/go-stellar-sdk"));
        assert!(!manifest.has_dependency("github.com/stellar/go"));

        let deps = parse_go_mod_dependencies(raw).expect("dependencies");
        assert_eq!(
            deps,
            vec![ManifestDep {
                name: "github.com/stellar/go-stellar-sdk".to_string(),
                section: "require".to_string(),
            }]
        );
    }

    #[test]
    fn reads_go_mod_block_require() {
        let raw = r#"
        module example.com/my/app

        go 1.22

        require (
            github.com/stellar/go-stellar-sdk v1.0.0
            github.com/stretchr/testify v1.8.4
        )
        "#;
        let manifest = parse_go_mod(raw).expect("valid go.mod block");
        assert_eq!(
            manifest.dependency_names,
            vec![
                "github.com/stellar/go-stellar-sdk",
                "github.com/stretchr/testify"
            ]
        );
        assert!(manifest.has_dependency("github.com/stellar/go-stellar-sdk"));
        assert!(manifest.has_dependency("github.com/stretchr/testify"));
    }

    #[test]
    fn reads_go_mod_with_comments_and_indirect() {
        let raw = r#"
        // Module definition
        module example.com/my/app // inline comment

        go 1.22

        // Direct & indirect dependencies
        require (
            // Stellar SDK
            github.com/stellar/go-stellar-sdk v1.0.0 // direct
            golang.org/x/crypto v0.14.0 // indirect
        )
        "#;
        let manifest = parse_go_mod(raw).expect("comments handled");
        assert_eq!(
            manifest.dependency_names,
            vec!["github.com/stellar/go-stellar-sdk", "golang.org/x/crypto"]
        );
    }

    #[test]
    fn reads_go_mod_multiple_require_blocks() {
        let raw = r#"
        module example.com/my/app

        require github.com/first/pkg v0.1.0

        require (
            github.com/stellar/go-stellar-sdk v1.0.0
        )

        require (
            github.com/second/pkg v0.2.0
        )
        "#;
        let manifest = parse_go_mod(raw).expect("multiple require blocks");
        assert_eq!(
            manifest.dependency_names,
            vec![
                "github.com/first/pkg",
                "github.com/second/pkg",
                "github.com/stellar/go-stellar-sdk",
            ]
        );
    }

    #[test]
    fn reads_go_mod_quoted_paths() {
        let raw = r#"
        module "example.com/my/app"

        require "github.com/stellar/go-stellar-sdk" "v1.0.0"

        require (
            `github.com/stretchr/testify` `v1.8.4`
        )
        "#;
        let manifest = parse_go_mod(raw).expect("quoted paths");
        assert_eq!(
            manifest.dependency_names,
            vec![
                "github.com/stellar/go-stellar-sdk",
                "github.com/stretchr/testify"
            ]
        );
    }

    #[test]
    fn go_mod_ignores_replace_and_exclude_directives() {
        let raw = r#"
        module example.com/my/app

        require (
            github.com/other/lib v1.0.0
        )

        replace github.com/other/lib => github.com/stellar/go-stellar-sdk v1.0.0

        replace (
            golang.org/x/sys => golang.org/x/sys v0.1.0
        )

        exclude (
            github.com/stellar/go-stellar-sdk v0.9.0
        )

        exclude github.com/other/lib v0.8.0
        "#;
        let manifest = parse_go_mod(raw).expect("replace and exclude ignored");
        assert_eq!(manifest.dependency_names, vec!["github.com/other/lib"]);
        assert!(!manifest.has_dependency("github.com/stellar/go-stellar-sdk"));
    }

    #[test]
    fn go_mod_empty_or_directives_only() {
        let raw = "module example.com/my/app\n\ngo 1.22\ntoolchain go1.22.1\n";
        let manifest = parse_go_mod(raw).expect("directives only");
        assert!(manifest.dependency_names.is_empty());

        let empty = parse_go_mod("").expect("empty is valid");
        assert!(empty.dependency_names.is_empty());
    }

    #[test]
    fn go_mod_malformed_unclosed_block() {
        let raw = "module example.com/my/app\n\nrequire (\n    github.com/stellar/go-stellar-sdk v1.0.0\n";
        let err = parse_go_mod(raw).expect_err("unclosed block must error");
        assert!(err.message.contains("unclosed require block"));
    }

    #[test]
    fn go_mod_malformed_unclosed_replace_block() {
        let raw = "module example.com/my/app\n\nreplace (\n    github.com/a => github.com/b v1.0\n";
        let err = parse_go_mod(raw).expect_err("unclosed replace block must error");
        assert!(err.message.contains("unclosed replace block"));
    }

    #[test]
    fn go_mod_malformed_stray_closing_paren() {
        let raw = "module example.com/my/app\n)\n";
        let err = parse_go_mod(raw).expect_err("stray closing paren");
        assert!(err.message.contains("unexpected closing parenthesis"));
    }

    #[test]
    fn go_mod_malformed_missing_version_single() {
        let raw = "module example.com/my/app\nrequire github.com/stellar/go-stellar-sdk\n";
        let err = parse_go_mod(raw).expect_err("missing version in single require");
        assert!(err
            .message
            .contains("require directive expects module path and version"));
    }

    #[test]
    fn go_mod_malformed_missing_version_block() {
        let raw =
            "module example.com/my/app\nrequire (\n    github.com/stellar/go-stellar-sdk\n)\n";
        let err = parse_go_mod(raw).expect_err("missing version in require block");
        assert!(err
            .message
            .contains("require block entry expects module path and version"));
    }

    #[test]
    fn go_mod_malformed_unclosed_quote() {
        let raw = "module example.com/my/app\nrequire \"github.com/stellar/go-stellar-sdk v1.0.0\n";
        let err = parse_go_mod(raw).expect_err("unclosed quote");
        assert!(err.message.contains("unclosed double quote"));
    }

    #[test]
    fn go_mod_malformed_unknown_directive() {
        let raw = "invalid_directive foo bar\n";
        let err = parse_go_mod(raw).expect_err("unknown directive");
        assert!(err.message.contains("unknown directive"));
    }

    #[test]
    fn go_mod_malformed_nested_blocks() {
        let raw = "require (\n    require (\n        foo v1.0\n    )\n)\n";
        let err = parse_go_mod(raw).expect_err("nested block");
        assert!(err.message.contains("nested block"));
    }

    #[test]
    fn go_mod_does_not_mutate_file() {
        let dir = super::super::test_support::temp_dir("manifest-go-mod");
        let path = dir.path.join("go.mod");
        let original_content = "module example.com/test\n\ngo 1.22\n\nrequire github.com/stellar/go-stellar-sdk v1.0.0\n";
        std::fs::write(&path, original_content).unwrap();

        let before_metadata = std::fs::metadata(&path).unwrap();
        let manifest = read_go_mod(&dir.path).expect("read go.mod");
        assert!(manifest.has_dependency("github.com/stellar/go-stellar-sdk"));

        let after_content = std::fs::read_to_string(&path).unwrap();
        let after_metadata = std::fs::metadata(&path).unwrap();

        assert_eq!(original_content, after_content);
        assert_eq!(before_metadata.len(), after_metadata.len());
    }
}
