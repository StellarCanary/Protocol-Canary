// Copyright 2023 StellarCanary
// SPDX-License-Identifier: Apache-2.0

use std::str::FromStr;

use anyhow::Result;
use clap::Parser;
use serde::Deserialize;
use stellar_strkey::PublicKey;

/// Network configuration and parsing utilities for Stellar protocol interactions.
pub mod network {
    use super::*;

    /// Parses a network name string into its corresponding `Network` enum variant.
    ///
    /// # Arguments
    /// * `network_name` - A string slice representing the network name (e.g., "public", "testnet", "future").
    ///
    /// # Returns
    /// `Result<Network>` - The parsed `Network` enum variant if successful.
    ///
    /// # Examples
    /// ```
    /// use canary_cli::network::{parse_network_name, Network};
    ///
    /// assert_eq!(parse_network_name("public").unwrap(), Network::Public);
    /// assert_eq!(parse_network_name("testnet").unwrap(), Network::Testnet);
    /// ```
    ///
    /// # Panics
    /// This function will panic if the input string is empty or contains only whitespace.
    ///
    /// # Errors
    /// Returns `Err` if the network name is not recognized or invalid.
    pub fn parse_network_name(network_name: &str) -> Result<Network> {
        let trimmed = network_name.trim();
        if trimmed.is_empty() {
            anyhow::bail!(r#"network name cannot be empty"#);
        }

        Network::from_str(trimmed)
    }

    /// Represents the Stellar network variants supported by the canary tooling.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Parser)]
    pub enum Network {
        /// Main Stellar public network.
        Public,

        /// Stellar test network.
        Testnet,

        /// Future network placeholder (reserved for future use).
        Future,
    }

    impl Network {
        /// Returns the network passphrase for the given network variant.
        pub fn passphrase(&self) -> &'static str {
            match self {
                Network::Public => "Public Global Stellar Network ; February 2015",
                Network::Testnet => "Test SDF Network ; September 2015",
                Network::Future => "Future Network",
            }
        }

        /// Returns the network identifier for the given network variant.
        pub fn id(&self) -> u32 {
            match self {
                Network::Public => 0x47131680,
                Network::Testnet => 0x11111111,
                Network::Future => 0x00000000,
            }
        }
    }

    impl FromStr for Network {
        type Err = anyhow::Error;

        fn from_str(s: &str) -> Result<Self> {
            match s.to_lowercase().as_str() {
                "public" => Ok(Network::Public),
                "testnet" => Ok(Network::Testnet),
                "future" => Ok(Network::Future),
                _ => anyhow::bail!(r#"unknown network name: {}"#, s),
            }
        }
    }
}
