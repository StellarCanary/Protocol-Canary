//! Resolving a `--network`/`--rpc-url` pair into a [`NetworkContext`].

use canary_core::NetworkName;

pub const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";
pub const FUTURENET_PASSPHRASE: &str = "Test SDF Future Network ; October 2022";
pub const MAINNET_PASSPHRASE: &str = "Public Global Stellar Network ; September 2015";
pub const TESTNET_DEFAULT_RPC_URL: &str = "https://soroban-testnet.stellar.org";

pub fn parse_network_name(name: &str) -> NetworkName {
    match name.to_ascii_lowercase().as_str() {
        "testnet" => NetworkName::Testnet,
        "mainnet" => NetworkName::Mainnet,
        "futurenet" => NetworkName::Futurenet,
        other => NetworkName::Custom(other.to_string()),
    }
}

pub fn default_passphrase(name: &NetworkName) -> Option<&'static str> {
    match name {
        NetworkName::Testnet => Some(TESTNET_PASSPHRASE),
        NetworkName::Futurenet => Some(FUTURENET_PASSPHRASE),
        NetworkName::Mainnet => Some(MAINNET_PASSPHRASE),
        NetworkName::Custom(_) => None,
    }
}

/// The default RPC URL for a network, when one is well-known.
///
/// There is deliberately no default for mainnet: the project's network
/// safety rule requires the user to explicitly opt into a mainnet
/// endpoint rather than the tool silently picking one for them.
pub fn default_rpc_url(name: &NetworkName) -> Option<&'static str> {
    match name {
        NetworkName::Testnet => Some(TESTNET_DEFAULT_RPC_URL),
        NetworkName::Futurenet | NetworkName::Mainnet | NetworkName::Custom(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_network_names_case_insensitively() {
        assert_eq!(parse_network_name("Testnet"), NetworkName::Testnet);
        assert_eq!(parse_network_name("MAINNET"), NetworkName::Mainnet);
        assert_eq!(parse_network_name("futurenet"), NetworkName::Futurenet);
    }

    #[test]
    fn unknown_names_become_custom() {
        assert_eq!(
            parse_network_name("my-standalone-network"),
            NetworkName::Custom("my-standalone-network".to_string())
        );
    }

    #[test]
    fn only_testnet_has_a_default_rpc_url() {
        assert!(default_rpc_url(&NetworkName::Testnet).is_some());
        assert!(default_rpc_url(&NetworkName::Mainnet).is_none());
        assert!(default_rpc_url(&NetworkName::Futurenet).is_none());
    }
}
