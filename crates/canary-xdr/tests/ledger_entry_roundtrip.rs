//! Lossless roundtrip coverage for LedgerEntry XDR.
//!
//! The property under test: for every supported entry type — Account,
//! Trustline, ContractData, and ContractCode —
//! `encode_ledger_entry(decode_ledger_entry(bytes)) == bytes`, byte for
//! byte. Four of the fixtures below are **real captures** taken from the
//! live Testnet RPC endpoint (`https://soroban-testnet.stellar.org`,
//! `getLedgerEntries`, on 2026-10-15; the network reported protocol 29 and
//! XDR v28.0 — see `docs/contracts/cf-04-verification-evidence.md` §5:
//! Protocol 29 introduced no new CAPs and runs XDR v28.0, so these entries
//! are valid for both the Protocol 28 and Protocol 29 eras). The RPC
//! returns raw `LedgerEntryData`; each capture is wrapped in the full
//! `LedgerEntry` struct with the `lastModifiedLedgerSeq` the endpoint
//! reported, which is the type this API decodes. Nothing else is altered.
//!
//! Negative cases assert that corrupted base64 and truncated buffers fail
//! with the typed `XdrDecodeError` and a byte offset, rather than panicking.

use base64::Engine as _;

use canary_xdr::{decode, decode_ledger_entry, encode_ledger_entry, XdrTypeName};
use stellar_xdr::{
    AccountEntry, AccountEntryExt, AccountId, BytesM, ContractCodeEntry, ContractCodeEntryExt,
    ContractDataDurability, ContractDataEntry, ContractId, ExtensionPoint, Hash, LedgerEntry,
    LedgerEntryData, PublicKey, ScAddress, ScSymbol, ScVal, SequenceNumber, String32, StringM,
    Thresholds, TrustLineAsset, Uint256, VecM,
};

/// Real capture: an account entry, from `getLedgerEntries` on Testnet
/// (lastModifiedLedgerSeq 5212226). Holds a BLND trustline among others.
const CAPTURED_ACCOUNT: &str = include_str!("data/account.b64");
/// Real capture: the account's BLND trustline (lastModifiedLedgerSeq
/// 4555757; balance 5000.0000000, limit i64::MAX, authorized).
const CAPTURED_TRUSTLINE: &str = include_str!("data/trustline.b64");
/// Real capture: a Pyth oracle's persistent ContractData instance entry
/// (lastModifiedLedgerSeq 4913492, contract 47582b04…f1a13e).
const CAPTURED_CONTRACT_DATA: &str = include_str!("data/contract_data.b64");
/// Real capture: the same contract's ContractCode entry
/// (lastModifiedLedgerSeq 4912092, WASM hash de796cd8…ed69c, 54,948 bytes
/// of code, `ContractCodeEntryExt::V1` carrying cost inputs — the newer
/// extension field whose lossless preservation this test also proves).
const CAPTURED_CONTRACT_CODE: &str = include_str!("data/contract_code.b64");

fn capture(name: &str, contents: &str) -> String {
    let trimmed = contents.trim();
    assert!(!trimmed.is_empty(), "capture file {name} must not be empty");
    trimmed.to_string()
}

fn account_id(seed: u8) -> AccountId {
    AccountId(PublicKey::PublicKeyTypeEd25519(Uint256([seed; 32])))
}

/// A compact, fully synthetic account entry for the constructed-entry
/// roundtrip (the captured one above is the real-world case).
fn constructed_account_entry() -> LedgerEntry {
    LedgerEntry {
        last_modified_ledger_seq: 100,
        data: LedgerEntryData::Account(AccountEntry {
            account_id: account_id(7),
            balance: 100_000_000,
            seq_num: SequenceNumber(1_234),
            num_sub_entries: 2,
            inflation_dest: Some(account_id(8)),
            flags: 0,
            home_domain: String32(
                StringM::try_from("canary.example".to_string()).expect("fits 32"),
            ),
            thresholds: Thresholds([1, 0, 0, 0]),
            signers: VecM::default(),
            ext: AccountEntryExt::V0,
        }),
        ext: stellar_xdr::LedgerEntryExt::V0,
    }
}

/// A synthetic ContractData entry with a Temporary-durability slot, to
/// cover the durability arm the captured (Persistent) entry does not.
fn constructed_contract_data_entry() -> LedgerEntry {
    LedgerEntry {
        last_modified_ledger_seq: 200,
        data: LedgerEntryData::ContractData(ContractDataEntry {
            ext: ExtensionPoint::V0,
            contract: ScAddress::Contract(ContractId(Hash([9; 32]))),
            key: ScVal::Symbol(ScSymbol(
                StringM::try_from("STATE".to_string()).expect("fits 32"),
            )),
            durability: ContractDataDurability::Temporary,
            val: ScVal::U32(7),
        }),
        ext: stellar_xdr::LedgerEntryExt::V0,
    }
}

/// A synthetic ContractCode entry: the smallest realistic WASM module
/// header, with the V0 extension (the captured entry covers V1).
fn constructed_contract_code_entry() -> LedgerEntry {
    LedgerEntry {
        last_modified_ledger_seq: 300,
        data: LedgerEntryData::ContractCode(ContractCodeEntry {
            ext: ContractCodeEntryExt::V0,
            hash: Hash([3; 32]),
            code: BytesM::try_from(vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00])
                .expect("fits BytesM"),
        }),
        ext: stellar_xdr::LedgerEntryExt::V0,
    }
}

/// The central property: decode then encode reproduces the input bytes
/// exactly, and the decoded value equals the original.
fn assert_lossless_roundtrip(entry: LedgerEntry) {
    let encoded = encode_ledger_entry(&entry).expect("encodes");
    let decoded = decode_ledger_entry(&encoded).expect("decodes");
    let re_encoded = encode_ledger_entry(&decoded).expect("re-encodes");

    assert_eq!(
        re_encoded, encoded,
        "encode(decode(bytes)) must equal bytes byte for byte"
    );
    assert_eq!(decoded, entry, "the decoded entry must equal the original");
}

// ---------------------------------------------------------------------------
// Real captures: byte-for-byte roundtrip of what a live network serves.
// ---------------------------------------------------------------------------

#[test]
fn a_real_captured_account_entry_roundtrips_byte_for_byte() {
    let base64 = capture("account", CAPTURED_ACCOUNT);
    let decoded = decode_ledger_entry(&base64).expect("decodes");
    assert!(matches!(decoded.data, LedgerEntryData::Account(_)));
    assert_eq!(decoded.last_modified_ledger_seq, 5_212_226);
    assert_eq!(
        encode_ledger_entry(&decoded).expect("re-encodes"),
        base64,
        "encode(decode(capture)) must equal the capture byte for byte"
    );
}

#[test]
fn a_real_captured_trustline_entry_roundtrips_byte_for_byte() {
    let base64 = capture("trustline", CAPTURED_TRUSTLINE);
    let decoded = decode_ledger_entry(&base64).expect("decodes");
    match &decoded.data {
        LedgerEntryData::Trustline(trustline) => {
            // BLND/GATALT… as served by the network.
            assert_eq!(trustline.balance, 50_000_000_000);
            assert_eq!(trustline.limit, i64::MAX);
            assert!(matches!(
                trustline.asset,
                TrustLineAsset::CreditAlphanum4(_)
            ));
        }
        other => panic!("expected a Trustline entry, got {other:?}"),
    }
    assert_eq!(
        encode_ledger_entry(&decoded).expect("re-encodes"),
        base64,
        "encode(decode(capture)) must equal the capture byte for byte"
    );
}

#[test]
fn a_real_captured_contract_data_entry_roundtrips_byte_for_byte() {
    let base64 = capture("contract_data", CAPTURED_CONTRACT_DATA);
    let decoded = decode_ledger_entry(&base64).expect("decodes");
    match &decoded.data {
        LedgerEntryData::ContractData(data) => {
            assert_eq!(data.durability, ContractDataDurability::Persistent);
            assert_eq!(
                data.contract,
                ScAddress::Contract(ContractId(Hash([
                    0x47, 0x58, 0x2b, 0x04, 0x6f, 0xa0, 0xc8, 0x9a, 0x19, 0x17, 0xfe, 0x32, 0xe0,
                    0x90, 0xbc, 0x0e, 0x36, 0x0a, 0xa9, 0xdc, 0xde, 0x14, 0xa9, 0xa4, 0x8a, 0x6a,
                    0xc4, 0x12, 0x40, 0xf1, 0xa1, 0x3e,
                ])))
            );
            assert!(
                matches!(data.val, ScVal::ContractInstance(_)),
                "the captured entry holds an initialized contract instance"
            );
        }
        other => panic!("expected a ContractData entry, got {other:?}"),
    }
    assert_eq!(
        encode_ledger_entry(&decoded).expect("re-encodes"),
        base64,
        "encode(decode(capture)) must equal the capture byte for byte"
    );
}

#[test]
fn a_real_captured_contract_code_entry_roundtrips_byte_for_byte() {
    let base64 = capture("contract_code", CAPTURED_CONTRACT_CODE);
    let decoded = decode_ledger_entry(&base64).expect("decodes");
    match &decoded.data {
        LedgerEntryData::ContractCode(code) => {
            assert_eq!(
                code.hash,
                Hash([
                    0xde, 0x79, 0x6c, 0xd8, 0x80, 0xad, 0x16, 0x9e, 0x2c, 0xc7, 0x44, 0x22, 0x6d,
                    0x0e, 0x75, 0x19, 0x96, 0x43, 0x3c, 0x7d, 0xe2, 0xf8, 0x48, 0xb0, 0xe9, 0x49,
                    0x0e, 0x0d, 0x2e, 0x1e, 0xd6, 0x9c,
                ])
            );
            assert_eq!(code.code.len(), 54_948, "the full WASM module is kept");
        }
        other => panic!("expected a ContractCode entry, got {other:?}"),
    }
    assert_eq!(
        encode_ledger_entry(&decoded).expect("re-encodes"),
        base64,
        "encode(decode(capture)) must equal the capture byte for byte"
    );
}

/// The captured ContractCode entry carries `ContractCodeEntryExt::V1` with
/// its cost inputs — a newer extension field. Lossless preservation of it
/// is the concrete form of "handles new Protocol 28/29 fields without data
/// loss".
#[test]
fn the_captured_contract_code_entry_preserves_its_v1_extension() {
    let base64 = capture("contract_code", CAPTURED_CONTRACT_CODE);
    let decoded = decode_ledger_entry(&base64).expect("decodes");
    let has_v1_ext = match &decoded.data {
        LedgerEntryData::ContractCode(code) => {
            matches!(code.ext, ContractCodeEntryExt::V1(_))
        }
        other => panic!("expected a ContractCode entry, got {other:?}"),
    };
    assert!(
        has_v1_ext,
        "this capture must exercise the ContractCodeEntryExt::V1 extension"
    );
}

// ---------------------------------------------------------------------------
// Constructed entries: coverage of arms the captures do not hit.
// ---------------------------------------------------------------------------

#[test]
fn a_constructed_account_entry_roundtrips() {
    assert_lossless_roundtrip(constructed_account_entry());
}

#[test]
fn a_constructed_temporary_contract_data_entry_roundtrips() {
    assert_lossless_roundtrip(constructed_contract_data_entry());
}

#[test]
fn a_constructed_contract_code_entry_roundtrips() {
    assert_lossless_roundtrip(constructed_contract_code_entry());
}

/// The fixture pipeline goes through `decode`/`encode` with a type name;
/// the typed entry points must agree with it so both paths roundtrip.
#[test]
fn the_typed_api_and_the_fixture_pipeline_agree() {
    let base64 = capture("account", CAPTURED_ACCOUNT);
    let via_fixture_path =
        decode(XdrTypeName::LedgerEntry, &base64).expect("decodes through the generic path");
    let via_typed_path = decode_ledger_entry(&base64).expect("decodes");
    match via_fixture_path {
        canary_xdr::DecodedValue::LedgerEntry(entry) => assert_eq!(*entry, via_typed_path),
        other => {
            panic!("decode with XdrTypeName::LedgerEntry must hold a ledger entry, got {other:?}")
        }
    }
}

// ---------------------------------------------------------------------------
// Negative tests: typed errors, byte offsets, no panics.
// ---------------------------------------------------------------------------

#[test]
fn corrupted_base64_returns_a_typed_error_at_offset_zero() {
    let err = decode_ledger_entry("%%% not base64 %%%").unwrap_err();
    assert!(
        matches!(err, canary_xdr::XdrDecodeError::InvalidBase64(_)),
        "expected InvalidBase64, got {err:?}"
    );
    assert_eq!(err.byte_offset(), 0);
    assert!(err.to_string().contains("base64"), "{err}");
}

/// A valid entry cut short must report `InvalidXdr` with the offset inside
/// the truncated buffer — the descriptive parse error the issue asks for.
#[test]
fn truncated_entry_data_returns_a_typed_error_with_a_byte_offset() {
    let base64 = capture("trustline", CAPTURED_TRUSTLINE);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&base64)
        .expect("capture is valid base64");
    let truncated = &bytes[..bytes.len() - 12];
    let truncated_base64 = base64::engine::general_purpose::STANDARD.encode(truncated);

    let err = decode_ledger_entry(&truncated_base64).unwrap_err();
    match &err {
        canary_xdr::XdrDecodeError::InvalidXdr {
            type_name,
            offset,
            message,
        } => {
            assert_eq!(*type_name, XdrTypeName::LedgerEntry);
            assert!(
                *offset > 0 && *offset <= truncated.len() as u64,
                "offset {offset} must point into the {}-byte truncated buffer",
                truncated.len()
            );
            assert!(!message.is_empty(), "the error must explain itself");
        }
        other => panic!("expected InvalidXdr, got {other:?}"),
    }
    assert!(err.to_string().contains("byte offset"), "{err}");
}

#[test]
fn trailing_bytes_after_a_complete_entry_return_a_typed_error() {
    let base64 = capture("account", CAPTURED_ACCOUNT);
    let mut bytes = base64::engine::general_purpose::STANDARD
        .decode(&base64)
        .expect("capture is valid base64");
    bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let with_trailing = base64::engine::general_purpose::STANDARD.encode(bytes);
    let err = decode_ledger_entry(&with_trailing).unwrap_err();
    match &err {
        canary_xdr::XdrDecodeError::InvalidXdr { message, .. } => {
            assert!(message.contains("trailing"), "message: {message}");
        }
        other => panic!("expected InvalidXdr, got {other:?}"),
    }
}

/// Bytes that are valid base64 but describe an unknown entry discriminant
/// must be rejected without panicking.
#[test]
fn an_invalid_entry_discriminant_is_rejected_without_panicking() {
    // Discriminant 99 is not any `LedgerEntryData` variant.
    let corrupt = base64::engine::general_purpose::STANDARD.encode([0, 0, 0, 99, 0, 0, 0, 0]);
    let err = decode_ledger_entry(&corrupt).unwrap_err();
    assert!(
        matches!(err, canary_xdr::XdrDecodeError::InvalidXdr { .. }),
        "expected InvalidXdr, got {err:?}"
    );
}

/// Completely malformed input — including the empty string — must surface
/// as a typed error, never a panic.
#[test]
fn malformed_input_never_panics() {
    for input in ["", "!!!!", "AAAA", "null", "AA==AA=="] {
        let _ = decode_ledger_entry(input);
    }
}

// ---------------------------------------------------------------------------
// ContractCode variants: bytecode fidelity and hash digests.
// ---------------------------------------------------------------------------

/// A minimal SHA-256 (FIPS 180-4) so the test can check the Wasm hash a
/// ContractCode entry carries without adding a dependency.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }

    let mut digest = [0u8; 32];
    for (chunk, word) in digest.chunks_exact_mut(4).zip(h) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// A synthetic Wasm module: the `\0asm` magic and version 1 header followed
/// by a payload that walks every byte value, so any byte-level corruption
/// in the roundtrip would be visible.
fn synthetic_wasm(payload_len: usize) -> Vec<u8> {
    let mut code = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    code.extend((0..payload_len).map(|i| (i % 256) as u8));
    code
}

fn contract_code_entry(code: Vec<u8>, ext: ContractCodeEntryExt) -> LedgerEntry {
    let hash = Hash(sha256(&code));
    LedgerEntry {
        last_modified_ledger_seq: 400,
        data: LedgerEntryData::ContractCode(ContractCodeEntry {
            ext,
            hash,
            code: BytesM::try_from(code).expect("fits BytesM"),
        }),
        ext: stellar_xdr::LedgerEntryExt::V0,
    }
}

#[test]
fn the_test_sha256_matches_the_fips_180_4_vectors() {
    assert_eq!(
        sha256(b"abc"),
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ]
    );
    assert_eq!(
        sha256(b""),
        [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ]
    );
}

/// The network's ContractCode hash is the SHA-256 of the Wasm bytes, so the
/// decoded bytecode of the real capture must hash to the entry's `hash`.
#[test]
fn the_captured_contract_code_hash_is_the_sha256_of_its_bytecode() {
    let base64 = capture("contract_code", CAPTURED_CONTRACT_CODE);
    let decoded = decode_ledger_entry(&base64).expect("decodes");
    match &decoded.data {
        LedgerEntryData::ContractCode(code) => {
            assert_eq!(&code.code[..4], b"\0asm", "the capture is a Wasm module");
            assert_eq!(Hash(sha256(&code.code)), code.hash);
        }
        other => panic!("expected a ContractCode entry, got {other:?}"),
    }
}

/// Synthetic ContractCode entries of several sizes — header only, under
/// and over one 256-byte cycle, and a few KiB — roundtrip with their
/// bytecode preserved bit for bit and their hash still matching the code.
#[test]
fn synthetic_contract_code_entries_preserve_bytecode_and_hash() {
    for payload_len in [0, 1, 255, 256, 257, 4_096] {
        let code = synthetic_wasm(payload_len);
        let entry = contract_code_entry(code.clone(), ContractCodeEntryExt::V0);

        let encoded = encode_ledger_entry(&entry).expect("encodes");
        let decoded = decode_ledger_entry(&encoded).expect("decodes");
        assert_eq!(
            encode_ledger_entry(&decoded).expect("re-encodes"),
            encoded,
            "payload {payload_len}: encode(decode(bytes)) must equal bytes"
        );
        match &decoded.data {
            LedgerEntryData::ContractCode(decoded_code) => {
                assert_eq!(
                    decoded_code.code.as_slice(),
                    code.as_slice(),
                    "payload {payload_len}: bytecode must survive bit for bit"
                );
                assert_eq!(decoded_code.hash, Hash(sha256(&code)));
            }
            other => panic!("expected a ContractCode entry, got {other:?}"),
        }
        assert_eq!(decoded, entry);
    }
}

/// The captured entry's V1 extension (cost inputs) moved onto synthetic
/// bytecode still roundtrips losslessly, covering V1 on a constructed entry.
#[test]
fn a_synthetic_contract_code_entry_with_the_v1_extension_roundtrips() {
    let base64 = capture("contract_code", CAPTURED_CONTRACT_CODE);
    let ext = match decode_ledger_entry(&base64).expect("decodes").data {
        LedgerEntryData::ContractCode(code) => code.ext,
        other => panic!("expected a ContractCode entry, got {other:?}"),
    };
    assert!(matches!(ext, ContractCodeEntryExt::V1(_)));
    assert_lossless_roundtrip(contract_code_entry(synthetic_wasm(64), ext));
}
