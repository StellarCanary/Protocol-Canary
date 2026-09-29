//! Resolving a `--network`/`--rpc-url` pair into a [`canary_core::NetworkContext`].

use canary_core::NetworkName;

pub const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";
/// The default Stellar network passphrase for Futurenet.
///
/// Stellar networks use a unique network passphrase as a cryptographic domain
/// separator when hashing and signing transactions. This ensures transactions
/// signed for Futurenet cannot be executed or replayed on Testnet, Mainnet,
/// or private networks.
///
/// Futurenet is the test network provided by the Stellar Development Foundation (SDF)
/// for previewing and testing experimental features and upcoming protocol releases,
/// such as Soroban smart contracts.
///
/// Used by [`default_passphrase`] when the network is [`NetworkName::Futurenet`].
///
/// # Examples
///
/// ```rust,ignore
/// use crate::network::{default_passphrase, FUTURENET_PASSPHRASE};
/// use canary_core::NetworkName;
///
/// assert_eq!(
///     default_passphrase(&NetworkName::Futurenet),
///     Some(FUTURENET_PASSPHRASE)
/// );
/// ```
pub const FUTURENET_PASSPHRASE: &str = "Test SDF Future Network ; October 2022";
pub const MAINNET_PASSPHRASE: &str = "Public Global Stellar Network ; September 2015";
pub const TESTNET_DEFAULT_RPC_URL: &str = "https://soroban-testnet.stellar.org";

/// Parses a network name string into a [`NetworkName`] enum variant.
///
/// This function performs a case-insensitive match against well-known networks.
/// It recognizes `"testnet"`, `"mainnet"`, and `"futurenet"`. If the input does
/// not match any of these known networks, it returns a [`NetworkName::Custom`]
/// variant containing the lowercased input string.
///
/// # Examples
///
/// ```
/// use canary_core::NetworkName;
/// use canary_cli::network::parse_network_name;
///
/// assert_eq!(parse_network_name("Testnet"), NetworkName::Testnet);
/// assert_eq!(parse_network_name("MAINNET"), NetworkName::Mainnet);
/// assert_eq!(parse_network_name("futurenet"), NetworkName::Futurenet);
///
/// // Unrecognized names are converted to lowercase and wrapped in Custom
/// assert_eq!(
///     parse_network_name("My-Local-Network"),
///     NetworkName::Custom("my-local-network".to_string())
/// );
/// ```
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
