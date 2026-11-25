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
    fn an_empty_part_between_parts_changes_the_digest() {
        let plain = sha256_hex(&[b"a", b"b"]);
        assert_ne!(sha256_hex(&[b"a", b"", b"b"]), plain);
        assert_ne!(sha256_hex(&[b"a", b"b", b""]), plain);
        assert_ne!(sha256_hex(&[b"", b"a", b"b"]), plain);
    }

    #[test]
    fn the_number_of_empty_parts_is_part_of_the_digest() {
        let none = sha256_hex(&[]);
        let one = sha256_hex(&[b""]);
        let two = sha256_hex(&[b"", b""]);
        assert_ne!(none, one);
        assert_ne!(one, two);
        assert_ne!(none, two);
        assert_ne!(sha256_hex(&[b"a", b"", b""]), sha256_hex(&[b"a", b""]));
    }

    #[test]
    fn the_same_bytes_split_at_different_boundaries_digest_differently() {
        let split_at_five = sha256_hex(&[b"hello", b"world"]);
        let split_at_four = sha256_hex(&[b"hell", b"oworld"]);
        let unsplit = sha256_hex(&[b"helloworld"]);
        assert_ne!(split_at_five, split_at_four);
        assert_ne!(split_at_five, unsplit);
        assert_ne!(split_at_four, unsplit);
    }

    #[test]
    fn one_part_that_looks_like_the_framed_form_of_two_parts_does_not_collide() {
        // The length prefix is what keeps `["ab", "c"]` apart from a single
        // part that happens to contain the bytes the framing would produce.
        let (first, second): (&[u8], &[u8]) = (b"ab", b"c");
        let mut framed = Vec::new();
        framed.extend_from_slice(&(first.len() as u64).to_le_bytes());
        framed.extend_from_slice(first);
        framed.extend_from_slice(&(second.len() as u64).to_le_bytes());
        framed.extend_from_slice(second);

        assert_ne!(sha256_hex(&[&framed]), sha256_hex(&[first, second]));
    }

    #[test]
    fn the_digest_does_not_depend_on_how_the_parts_are_allocated() {
        let reference = sha256_hex(&[b"canary", b"", b"pack"]);

        let owned_a = b"canary".to_vec();
        let owned_b: Vec<u8> = Vec::new();
        let owned_c = String::from("pack").into_bytes();
        assert_eq!(sha256_hex(&[&owned_a, &owned_b, &owned_c]), reference);

        // Parts cut out of one buffer, and a boxed slice, hash the same.
        let joined = b"canarypack".to_vec();
        assert_eq!(
            sha256_hex(&[&joined[..6], &joined[6..6], &joined[6..]]),
            reference
        );
        let boxed: Box<[u8]> = b"pack".to_vec().into_boxed_slice();
        assert_eq!(sha256_hex(&[b"canary", b"", &boxed]), reference);

        // Hashing the same input twice gives the same digest.
        assert_eq!(sha256_hex(&[b"canary", b"", b"pack"]), reference);
    }

    #[test]
    fn every_input_yields_sixty_four_lowercase_hex_characters() {
        let all_bytes: Vec<u8> = (0..=u8::MAX).collect();
        let large = vec![0xab_u8; 100_000];
        let inputs: Vec<Vec<&[u8]>> = vec![
            vec![],
            vec![b""],
            vec![b"", b"", b""],
            vec![b"a"],
            vec![b"a", b"", b"b"],
            vec![&all_bytes],
            vec![&[0x00], &[0xff]],
            vec![&large],
        ];

        for parts in inputs {
            let digest = sha256_hex(&parts);
            assert_eq!(digest.len(), 64, "{} parts", parts.len());
            assert!(
                digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "not lowercase hex: {digest}"
            );
        }
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
