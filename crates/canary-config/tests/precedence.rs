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

/// Environment variable configuration overrides (`STELLAR_CANARY_*`).
#[derive(Debug, Clone, Default, PartialEq)]
struct EnvOverrides {
    network: Option<String>,
    protocol: Option<u32>,
    rpc_url: Option<String>,
    fixtures_dir: Option<PathBuf>,
    rpc_timeout: Option<u64>,
}

impl EnvOverrides {
    fn from_env() -> Self {
        Self {
            network: std::env::var("STELLAR_CANARY_NETWORK")
                .ok()
                .filter(|s| !s.is_empty()),
            protocol: std::env::var("STELLAR_CANARY_PROTOCOL")
                .ok()
                .and_then(|v| v.parse().ok()),
            rpc_url: std::env::var("STELLAR_CANARY_RPC_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            fixtures_dir: std::env::var("STELLAR_CANARY_FIXTURES_DIR")
                .ok()
                .map(PathBuf::from),
            rpc_timeout: std::env::var("STELLAR_CANARY_TIMEOUT")
                .or_else(|_| std::env::var("STELLAR_CANARY_RPC_TIMEOUT"))
                .ok()
                .and_then(|v| v.parse().ok()),
        }
    }
}

/// Extended CLI arguments supporting protocol and timeout alongside network, rpc_url, and fixtures_dir.
#[derive(Debug, Clone, Default, PartialEq)]
struct ExtendedCliOverrides {
    cli: CliOverrides,
    protocol: Option<u32>,
    rpc_timeout: Option<u64>,
}

/// Full resolved configuration reflecting the three-tier resolution hierarchy:
/// CLI arguments > environment variables (`STELLAR_CANARY_*`) > `.stellar-canary.toml` > defaults.
#[derive(Debug, Clone, PartialEq)]
struct TieredResolvedConfig {
    network: String,
    protocol: u32,
    rpc_url: Option<String>,
    fixtures_dir: PathBuf,
    rpc_timeout: u64,
}

const DEFAULT_TIMEOUT: u64 = 10;
const DEFAULT_PROTOCOL_VERSION: u32 = 28;

fn resolve_three_tier(
    config: &ConfigFile,
    env: &EnvOverrides,
    cli: &ExtendedCliOverrides,
) -> TieredResolvedConfig {
    let effective_network = cli
        .cli
        .network
        .clone()
        .or_else(|| env.network.clone())
        .or_else(|| config.network.clone())
        .unwrap_or_else(|| DEFAULT_NETWORK.to_string());

    let effective_protocol = cli
        .protocol
        .or(env.protocol)
        .unwrap_or(if config.protocol != 0 {
            config.protocol
        } else {
            DEFAULT_PROTOCOL_VERSION
        });

    let effective_rpc_url = cli
        .cli
        .rpc_url
        .clone()
        .or_else(|| env.rpc_url.clone())
        .or_else(|| config.rpc_url.clone());

    let effective_fixtures_dir = cli
        .cli
        .fixtures_dir
        .clone()
        .or_else(|| env.fixtures_dir.clone())
        .or_else(|| config.fixtures_dir.clone())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FIXTURES_DIR));

    let effective_timeout = cli
        .rpc_timeout
        .or(env.rpc_timeout)
        .unwrap_or(DEFAULT_TIMEOUT);

    TieredResolvedConfig {
        network: effective_network,
        protocol: effective_protocol,
        rpc_url: effective_rpc_url,
        fixtures_dir: effective_fixtures_dir,
        rpc_timeout: effective_timeout,
    }
}

#[test]
fn cli_arguments_take_precedence_over_env_vars_and_config_file() {
    let config = ConfigFile {
        network: Some("mainnet".to_string()),
        protocol: 20,
        rpc_url: Some("https://config.example.invalid".to_string()),
        fixtures_dir: Some(PathBuf::from("config-fixtures")),
        ..ConfigFile::default()
    };
    let env = EnvOverrides {
        network: Some("futurenet".to_string()),
        protocol: Some(25),
        rpc_url: Some("https://env.example.invalid".to_string()),
        fixtures_dir: Some(PathBuf::from("env-fixtures")),
        rpc_timeout: Some(30),
    };
    let cli = ExtendedCliOverrides {
        cli: CliOverrides {
            network: Some("testnet".to_string()),
            rpc_url: Some("https://cli.example.invalid".to_string()),
            fixtures_dir: Some(PathBuf::from("cli-fixtures")),
        },
        protocol: Some(28),
        rpc_timeout: Some(5),
    };

    let resolved = resolve_three_tier(&config, &env, &cli);
    assert_eq!(resolved.network, "testnet");
    assert_eq!(resolved.protocol, 28);
    assert_eq!(
        resolved.rpc_url.as_deref(),
        Some("https://cli.example.invalid")
    );
    assert_eq!(resolved.fixtures_dir, PathBuf::from("cli-fixtures"));
    assert_eq!(resolved.rpc_timeout, 5);
}

#[test]
fn env_vars_take_precedence_over_config_file_when_cli_absent() {
    let config = ConfigFile {
        network: Some("mainnet".to_string()),
        protocol: 20,
        rpc_url: Some("https://config.example.invalid".to_string()),
        fixtures_dir: Some(PathBuf::from("config-fixtures")),
        ..ConfigFile::default()
    };
    let env = EnvOverrides {
        network: Some("futurenet".to_string()),
        protocol: Some(25),
        rpc_url: Some("https://env.example.invalid".to_string()),
        fixtures_dir: Some(PathBuf::from("env-fixtures")),
        rpc_timeout: Some(30),
    };
    let cli = ExtendedCliOverrides::default();

    let resolved = resolve_three_tier(&config, &env, &cli);
    assert_eq!(resolved.network, "futurenet");
    assert_eq!(resolved.protocol, 25);
    assert_eq!(
        resolved.rpc_url.as_deref(),
        Some("https://env.example.invalid")
    );
    assert_eq!(resolved.fixtures_dir, PathBuf::from("env-fixtures"));
    assert_eq!(resolved.rpc_timeout, 30);
}

#[test]
fn config_file_values_take_precedence_over_defaults_when_cli_and_env_absent() {
    let config = ConfigFile {
        network: Some("mainnet".to_string()),
        protocol: 22,
        rpc_url: Some("https://config.example.invalid".to_string()),
        fixtures_dir: Some(PathBuf::from("config-fixtures")),
        ..ConfigFile::default()
    };
    let env = EnvOverrides::default();
    let cli = ExtendedCliOverrides::default();

    let resolved = resolve_three_tier(&config, &env, &cli);
    assert_eq!(resolved.network, "mainnet");
    assert_eq!(resolved.protocol, 22);
    assert_eq!(
        resolved.rpc_url.as_deref(),
        Some("https://config.example.invalid")
    );
    assert_eq!(resolved.fixtures_dir, PathBuf::from("config-fixtures"));
    assert_eq!(resolved.rpc_timeout, DEFAULT_TIMEOUT);
}

#[test]
fn three_tier_precedence_applied_independently_per_parameter() {
    let config = ConfigFile {
        network: Some("mainnet".to_string()),
        protocol: 20,
        rpc_url: Some("https://config.example.invalid".to_string()),
        fixtures_dir: Some(PathBuf::from("config-fixtures")),
        ..ConfigFile::default()
    };
    let env = EnvOverrides {
        network: Some("futurenet".to_string()),
        protocol: Some(25),
        rpc_url: None,
        fixtures_dir: Some(PathBuf::from("env-fixtures")),
        rpc_timeout: Some(45),
    };
    let cli = ExtendedCliOverrides {
        cli: CliOverrides {
            network: Some("testnet".to_string()),
            rpc_url: None,
            fixtures_dir: None,
        },
        protocol: None,
        rpc_timeout: Some(3),
    };

    let resolved = resolve_three_tier(&config, &env, &cli);
    assert_eq!(resolved.network, "testnet");
    assert_eq!(resolved.protocol, 25);
    assert_eq!(
        resolved.rpc_url.as_deref(),
        Some("https://config.example.invalid")
    );
    assert_eq!(resolved.fixtures_dir, PathBuf::from("env-fixtures"));
    assert_eq!(resolved.rpc_timeout, 3);
}

#[test]
fn real_process_env_vars_parsed_and_prioritized() {
    struct EnvGuard(&'static str);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }

    std::env::set_var("STELLAR_CANARY_NETWORK", "env-network");
    let _g1 = EnvGuard("STELLAR_CANARY_NETWORK");
    std::env::set_var("STELLAR_CANARY_PROTOCOL", "26");
    let _g2 = EnvGuard("STELLAR_CANARY_PROTOCOL");
    std::env::set_var("STELLAR_CANARY_TIMEOUT", "42");
    let _g3 = EnvGuard("STELLAR_CANARY_TIMEOUT");

    let parsed_env = EnvOverrides::from_env();
    assert_eq!(parsed_env.network.as_deref(), Some("env-network"));
    assert_eq!(parsed_env.protocol, Some(26));
    assert_eq!(parsed_env.rpc_timeout, Some(42));

    let config = ConfigFile {
        network: Some("file-network".to_string()),
        protocol: 22,
        ..ConfigFile::default()
    };

    // Env overrides config file
    let no_cli = ExtendedCliOverrides::default();
    let res1 = resolve_three_tier(&config, &parsed_env, &no_cli);
    assert_eq!(res1.network, "env-network");
    assert_eq!(res1.protocol, 26);
    assert_eq!(res1.rpc_timeout, 42);

    // CLI overrides env var and config file
    let with_cli = ExtendedCliOverrides {
        cli: CliOverrides {
            network: Some("cli-network".to_string()),
            ..CliOverrides::default()
        },
        protocol: Some(29),
        rpc_timeout: Some(7),
    };
    let res2 = resolve_three_tier(&config, &parsed_env, &with_cli);
    assert_eq!(res2.network, "cli-network");
    assert_eq!(res2.protocol, 29);
    assert_eq!(res2.rpc_timeout, 7);
}
