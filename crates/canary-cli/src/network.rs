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
/// The canonical passphrase for the Stellar public network ("Public Global Stellar Network : November 2015").
/// It is used for transaction signing on Mainnet.
/// It represents network identification data only and does not configure RPC endpoints.
pub const MAINNET_PASSPHRASE: &str = "Public Global Stellar Network ; September 2015";
/// The default RPC endpoint for Stellar Testnet.
///
/// Testnet is the public test network run by the Stellar Development
/// Foundation (SDF). This is its public Soroban RPC endpoint, the one the
/// project's shipped Protocol 28 fixtures were verified against; see
/// `docs/protocol-28.md`.
///
/// # Usage
///
/// This is a *default*, not a mandatory endpoint. It is returned by
/// [`default_rpc_url`] for [`NetworkName::Testnet`] and is the fallback the
/// `check` command uses when `--rpc-url` is not passed (the `--network`
/// default is `testnet`, so it applies out of the box). An explicit
/// `--rpc-url` always wins, so callers can point a `testnet` run at a local
/// or self-hosted Soroban node instead.
///
/// Testnet is the only network with a built-in default. For mainnet,
/// futurenet, and any custom network, [`default_rpc_url`] returns `None` and
/// `check` fails with a configuration error asking for `--rpc-url`; that is
/// deliberate, so results are never silently attributed to an endpoint the
/// user did not choose.
///
/// The endpoint's identity is checked against the run's assumptions
/// (`canary_rpc::validate_network_info`): its passphrase is compared to
/// [`TESTNET_PASSPHRASE`], so pointing `--network testnet` at a different
/// network is a configuration error rather than a silently wrong report.
///
/// This constant cannot panic. It performs no request and has no validation
/// of its own; reachability is only discovered later, when the RPC client
/// contacts the endpoint (reported per-fixture as an execution error, not a
/// panic).
///
/// # Examples
///
/// ```rust,ignore
/// use crate::network::{default_rpc_url, TESTNET_DEFAULT_RPC_URL};
/// use canary_core::NetworkName;
///
/// assert_eq!(
///     default_rpc_url(&NetworkName::Testnet),
///     Some(TESTNET_DEFAULT_RPC_URL)
/// );
/// ```
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

/// The well-known network passphrase for `name`, when one exists.
///
/// A passphrase is a network's published identity string — the same
/// value an RPC endpoint reports from `getNetwork`. The CLI uses this
/// one to confirm that a user-supplied `--rpc-url` really is the
/// network named by `--network` before any result is attributed to it
/// (see `canary_rpc::validate_network_info`). Pointing `--network
/// testnet` at a mainnet endpoint has to surface as a configuration
/// error, not as a silently mislabelled report.
///
/// The mapping is fixed:
///
/// - `NetworkName::Testnet` resolves to [`TESTNET_PASSPHRASE`],
///   `NetworkName::Futurenet` to [`FUTURENET_PASSPHRASE`], and
///   `NetworkName::Mainnet` to [`MAINNET_PASSPHRASE`].
/// - `NetworkName::Custom` resolves to `None`: a custom network has no
///   single published passphrase, and the name it carries is a label,
///   not a passphrase, so guessing one would turn a real mismatch into a
///   silent pass.
///
/// Unlike [`default_rpc_url`], mainnet *does* have a default here. That
/// safety rule governs which endpoint a run may use — mainnet requires
/// an explicit `--rpc-url` — not which network's identity is recognized
/// once the user has supplied one.
///
/// The result borrows a `'static` constant rather than the argument, so
/// it outlives the borrow of `name`; callers wanting an owned
/// [`String`] copy it. This is a pure lookup: no I/O, no allocation, and
/// no way to fail or panic.
///
/// # Examples
///
/// ```
/// # use canary_core::NetworkName;
/// use crate::network::default_passphrase;
///
/// assert_eq!(
///     default_passphrase(&NetworkName::Testnet),
///     Some("Test SDF Network ; September 2015")
/// );
/// assert_eq!(default_passphrase(&NetworkName::Custom("local".into())), None);
/// ```
///
/// # Failure conditions
///
/// `None` is the only way this can go wrong, and it is not an error: it
/// means "no well-known passphrase for this network", never "lookup
/// failed". Callers passing the result to
/// `canary_rpc::validate_network_info` must map `None` to `""`, because
/// an empty expected passphrase is precisely what skips the network
/// comparison there. Any other substitute would be compared against the
/// endpoint's real passphrase, which cannot match, failing every
/// custom-network run as a spurious `canary_rpc::RpcError::NetworkMismatch`.
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

    // Mirrors the `default_passphrase` rustdoc example. Doc tests do not
    // run for this crate's binary target, so the documented mapping is
    // asserted here instead.
    #[test]
    fn well_known_networks_resolve_to_their_passphrase_and_custom_ones_do_not() {
        assert_eq!(
            default_passphrase(&NetworkName::Testnet),
            Some("Test SDF Network ; September 2015")
        );
        assert_eq!(
            default_passphrase(&NetworkName::Futurenet),
            Some("Test SDF Future Network ; October 2022")
        );
        assert_eq!(
            default_passphrase(&NetworkName::Mainnet),
            Some("Public Global Stellar Network ; September 2015")
        );
        assert_eq!(
            default_passphrase(&NetworkName::Custom("local".into())),
            None
        );
    }
}
