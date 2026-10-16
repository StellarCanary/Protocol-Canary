//! Configuration precedence tests (issue #474): for `network`, `rpc_url`
//! and `fixtures_dir`, an explicitly passed CLI flag overrides the
//! `.stellar-canary.toml` setting, which overrides the built-in default;
//! with neither source the built-in default applies.

use std::path::PathBuf;

use canary_config::{
    resolve_settings, CliOverrides, ConfigFile, ResolvedSettings, DEFAULT_FIXTURES_DIR,
    DEFAULT_NETWORK,
};

fn config_with(
    network: Option<&str>,
    rpc_url: Option<&str>,
    fixtures_dir: Option<&str>,
) -> ConfigFile {
    ConfigFile {
        network: network.map(str::to_string),
        rpc_url: rpc_url.map(str::to_string),
        fixtures_dir: fixtures_dir.map(PathBuf::from),
        ..ConfigFile::default()
    }
}

#[test]
fn built_in_defaults_apply_when_neither_source_provides_a_value() {
    let resolved = resolve_settings(&ConfigFile::default(), CliOverrides::default());
    assert_eq!(
        resolved,
        ResolvedSettings {
            network: DEFAULT_NETWORK.to_string(),
            rpc_url: None,
            fixtures_dir: PathBuf::from(DEFAULT_FIXTURES_DIR),
        }
    );
    assert_eq!(resolved.network, "testnet");
    assert_eq!(resolved.fixtures_dir, PathBuf::from("fixtures"));
}

#[test]
fn config_file_values_override_the_built_in_defaults() {
    let config = config_with(
        Some("mainnet"),
        Some("https://rpc.example.invalid"),
        Some("custom-fixtures"),
    );
    let resolved = resolve_settings(&config, CliOverrides::default());
    assert_eq!(resolved.network, "mainnet");
    assert_eq!(
        resolved.rpc_url.as_deref(),
        Some("https://rpc.example.invalid")
    );
    assert_eq!(resolved.fixtures_dir, PathBuf::from("custom-fixtures"));
}

#[test]
fn explicit_cli_values_override_the_config_file() {
    let config = config_with(
        Some("mainnet"),
        Some("https://rpc.example.invalid"),
        Some("config-fixtures"),
    );
    let resolved = resolve_settings(
        &config,
        CliOverrides {
            network: Some("testnet".to_string()),
            rpc_url: Some("https://cli.example.invalid".to_string()),
            fixtures_dir: Some(PathBuf::from("cli-fixtures")),
        },
    );
    assert_eq!(resolved.network, "testnet");
    assert_eq!(
        resolved.rpc_url.as_deref(),
        Some("https://cli.example.invalid")
    );
    assert_eq!(resolved.fixtures_dir, PathBuf::from("cli-fixtures"));
}

#[test]
fn precedence_is_applied_per_key() {
    // Only `--network` is passed: `rpc_url` and `fixtures_dir` still come
    // from the configuration file, and the untouched keys do not fall to
    // the CLI or the defaults.
    let config = config_with(
        Some("futurenet"),
        Some("https://futurenet.example.invalid"),
        Some("config-fixtures"),
    );
    let resolved = resolve_settings(
        &config,
        CliOverrides {
            network: Some("mainnet".to_string()),
            rpc_url: None,
            fixtures_dir: None,
        },
    );
    assert_eq!(resolved.network, "mainnet");
    assert_eq!(
        resolved.rpc_url.as_deref(),
        Some("https://futurenet.example.invalid")
    );
    assert_eq!(resolved.fixtures_dir, PathBuf::from("config-fixtures"));
}

#[test]
fn partial_sources_mix_with_the_built_in_defaults() {
    // Config file sets only `network`; the other keys fall through to the
    // defaults (no RPC URL, `fixtures`).
    let config = config_with(Some("mainnet"), None, None);
    let resolved = resolve_settings(&config, CliOverrides::default());
    assert_eq!(resolved.network, "mainnet");
    assert_eq!(resolved.rpc_url, None);
    assert_eq!(resolved.fixtures_dir, PathBuf::from(DEFAULT_FIXTURES_DIR));

    // CLI sets only `fixtures_dir`; the rest falls through to the config
    // file and then the defaults.
    let config = config_with(Some("futurenet"), None, None);
    let resolved = resolve_settings(
        &config,
        CliOverrides {
            network: None,
            rpc_url: None,
            fixtures_dir: Some(PathBuf::from("cli-fixtures")),
        },
    );
    assert_eq!(resolved.network, "futurenet");
    assert_eq!(resolved.rpc_url, None);
    assert_eq!(resolved.fixtures_dir, PathBuf::from("cli-fixtures"));
}

#[test]
fn an_unusable_config_url_is_overridden_cleanly_by_a_valid_cli_flag() {
    // The configuration file's value is taken verbatim and never validated
    // at resolution time, so a valid `--rpc-url` wins without the bad
    // recorded value causing an error of its own.
    let config = config_with(None, Some("not a url"), None);
    let resolved = resolve_settings(
        &config,
        CliOverrides {
            network: None,
            rpc_url: Some("https://soroban-testnet.stellar.org".to_string()),
            fixtures_dir: None,
        },
    );
    assert_eq!(
        resolved.rpc_url.as_deref(),
        Some("https://soroban-testnet.stellar.org")
    );
}

#[test]
fn the_new_settings_parse_from_toml_and_unknown_keys_still_fail() {
    let parsed: ConfigFile = toml::from_str(
        r##"
version = 1
protocol = 28
network = "mainnet"
rpc_url = "https://rpc.example.invalid"
fixtures_dir = "my-fixtures"
"##,
    )
    .expect("the new keys parse");
    assert_eq!(parsed.network.as_deref(), Some("mainnet"));
    assert_eq!(
        parsed.rpc_url.as_deref(),
        Some("https://rpc.example.invalid")
    );
    assert_eq!(
        parsed.fixtures_dir.as_deref(),
        Some(std::path::Path::new("my-fixtures"))
    );

    // A file without them still parses (they are optional) …
    let minimal: ConfigFile =
        toml::from_str("version = 1\nprotocol = 28\n").expect("minimal parses");
    assert_eq!(minimal.network, None);
    assert_eq!(minimal.rpc_url, None);
    assert_eq!(minimal.fixtures_dir, None);

    // … and a misspelled key is still rejected outright.
    let misspelled: Result<ConfigFile, _> =
        toml::from_str("version = 1\nprotocol = 28\nnetworks = \"mainnet\"\n");
    assert!(misspelled.is_err(), "misspelled key must be rejected");
}
