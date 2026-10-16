//! Machine-readable skip reason codes shared by the planner and the
//! reporters (CF-01 `skipped[].code`).
//!
//! A skip is recorded twice in a report: as free-form prose in
//! `skipped[].reason` for humans, and as a stable code in
//! `skipped[].code` for tooling. CI summaries key off the code so they can
//! group skips without parsing sentences.

use std::fmt;

/// The stable, machine-readable reason a fixture was skipped.
///
/// Codes are written in `SCREAMING_SNAKE_CASE` and are an *open* string
/// field under CF-01's additive rule: a consumer that reads a code it does
/// not know must treat it as [`SkipCode::Unknown`] rather than reject the
/// report, and a report written before the field existed reads back as
/// [`SkipCode::Unknown`] too. [`SkipCode`] itself never renders as an empty
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SkipCode {
    /// The fixture targets a protocol version this run does not target.
    UnsupportedProtocol,
    /// The project does not declare a capability the fixture requires.
    MissingCapability,
    /// The observed network identity does not match the configured network.
    ///
    /// Reserved: no planner path skips for a network-identity reason today
    /// (a passphrase mismatch is a configuration error and an observed
    /// protocol mismatch is a warning), but the code is part of the
    /// standard vocabulary so consumers can key off it from day one.
    NetworkMismatch,
    /// The fixture's surface is switched off in configuration.
    SurfaceDisabled,
    /// Safe fallback: the reason is not one this build recognizes, or the
    /// report predates the `code` field. Never an empty string.
    #[default]
    Unknown,
}

impl SkipCode {
    /// The report spelling of this code. Never empty.
    pub const fn as_str(self) -> &'static str {
        match self {
            SkipCode::UnsupportedProtocol => "UNSUPPORTED_PROTOCOL",
            SkipCode::MissingCapability => "MISSING_CAPABILITY",
            SkipCode::NetworkMismatch => "NETWORK_MISMATCH",
            SkipCode::SurfaceDisabled => "SURFACE_DISABLED",
            SkipCode::Unknown => "UNKNOWN",
        }
    }

    /// Reads a code from an untrusted string — a `skipped[].code` value in
    /// a saved report, or a reason this build did not produce.
    ///
    /// Anything unrecognized, including the empty string, maps to
    /// [`SkipCode::Unknown`] instead of failing or yielding an empty code,
    /// so a report from a newer version (or a corrupted one) still parses
    /// and still renders a usable `code`.
    pub fn parse(value: &str) -> Self {
        match value {
            "UNSUPPORTED_PROTOCOL" => SkipCode::UnsupportedProtocol,
            "MISSING_CAPABILITY" => SkipCode::MissingCapability,
            "NETWORK_MISMATCH" => SkipCode::NetworkMismatch,
            "SURFACE_DISABLED" => SkipCode::SurfaceDisabled,
            _ => SkipCode::Unknown,
        }
    }
}

impl fmt::Display for SkipCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_the_standard_screaming_snake_spelling() {
        assert_eq!(
            SkipCode::UnsupportedProtocol.as_str(),
            "UNSUPPORTED_PROTOCOL"
        );
        assert_eq!(SkipCode::MissingCapability.as_str(), "MISSING_CAPABILITY");
        assert_eq!(SkipCode::NetworkMismatch.as_str(), "NETWORK_MISMATCH");
        assert_eq!(SkipCode::SurfaceDisabled.as_str(), "SURFACE_DISABLED");
        assert_eq!(SkipCode::Unknown.as_str(), "UNKNOWN");
    }

    #[test]
    fn no_code_is_ever_an_empty_string() {
        for code in [
            SkipCode::UnsupportedProtocol,
            SkipCode::MissingCapability,
            SkipCode::NetworkMismatch,
            SkipCode::SurfaceDisabled,
            SkipCode::Unknown,
        ] {
            assert!(!code.as_str().is_empty(), "{code:?} must not be empty");
            assert!(
                !code.to_string().is_empty(),
                "{code:?} must not display empty"
            );
        }
    }

    #[test]
    fn canonical_spellings_round_trip_through_parse() {
        for code in [
            SkipCode::UnsupportedProtocol,
            SkipCode::MissingCapability,
            SkipCode::NetworkMismatch,
            SkipCode::SurfaceDisabled,
            SkipCode::Unknown,
        ] {
            assert_eq!(SkipCode::parse(code.as_str()), code);
        }
    }

    #[test]
    fn an_unknown_reason_falls_back_to_unknown_instead_of_empty() {
        // The negative case for the fallback: prose, a code from a newer
        // build, and the empty string all resolve to the safe fallback.
        assert_eq!(
            SkipCode::parse("no RPC endpoint configured"),
            SkipCode::Unknown
        );
        assert_eq!(SkipCode::parse("FUTURE_SKIP_CODE"), SkipCode::Unknown);
        assert_eq!(SkipCode::parse(""), SkipCode::Unknown);
        assert_eq!(SkipCode::default(), SkipCode::Unknown);
        assert_eq!(SkipCode::Unknown.as_str(), "UNKNOWN");
    }

    #[test]
    fn display_matches_the_report_spelling() {
        assert_eq!(SkipCode::SurfaceDisabled.to_string(), "SURFACE_DISABLED");
        assert_eq!(format!("{}", SkipCode::Unknown), "UNKNOWN");
    }
}
