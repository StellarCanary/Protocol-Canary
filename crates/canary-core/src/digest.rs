//! SHA-256 helpers shared by the cache and the fixture loader.

use sha2::{Digest, Sha256};

/// Hashes `parts` as one message and returns the lowercase hex digest.
///
/// Each part is framed with its length (8 bytes, little endian) before its
/// bytes, so the boundary between parts is part of what is hashed:
/// `["ab", "c"]` and `["a", "bc"]` produce different digests.
pub fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_lowercase_hex_of_the_expected_length() {
        let digest = sha256_hex(&[b"abc"]);
        assert_eq!(digest.len(), 64);
        assert!(digest
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    #[test]
    fn part_boundaries_are_part_of_the_digest() {
        assert_ne!(sha256_hex(&[b"ab", b"c"]), sha256_hex(&[b"a", b"bc"]));
        assert_ne!(sha256_hex(&[b"a", b""]), sha256_hex(&[b"a"]));
    }

    #[test]
    fn digest_is_stable() {
        // Pinned so an accidental change to the framing is caught: cache
        // keys and fixture digests are compared across runs and versions.
        // The first value is SHA-256 of the 8-byte little-endian length 6
        // followed by "canary"; the second is SHA-256 of no bytes.
        assert_eq!(
            sha256_hex(&[b"canary"]),
            "db18b568d69c70a736ce7ee8a4a165653356a441d1aeacf7e1b793dd44793a7f"
        );
        assert_eq!(
            sha256_hex(&[]),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
