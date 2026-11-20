//! Integration and roundtrip tests for LedgerHeader and LedgerCloseMeta XDR.
//!
//! Validates:
//! - Type name parsing and display formatting.
//! - Decode and encode roundtrip fidelity (byte-for-byte).
//! - Protocol 30 millisecond timestamp variants (SignedMs, EmptyTxSetMs) embedded in ledger headers.
//! - LedgerCloseMeta V0, V1, and V2 variants carrying ledger headers.
//! - JSON serialization via `to_json()`.
//! - Typed error handling for corrupted base64, truncated payloads, and trailing bytes.
//! - Compatibility with all XdrFixture assertion types via `DefaultXdrRunner`.

use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

use canary_core::{
    CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext, NetworkName,
    ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_xdr::{
    decode, decode_ledger_close_meta, decode_ledger_header, encode, encode_ledger_close_meta,
    encode_ledger_header, DecodedValue, DefaultXdrRunner, XdrDecodeError, XdrFixture, XdrRunner,
    XdrTypeName,
};
use stellar_xdr::{
    Hash, LedgerCloseMeta, LedgerCloseMetaV0, LedgerCloseMetaV1, LedgerCloseMetaV2,
    LedgerCloseValueSignature, LedgerHeader, LedgerHeaderExt, LedgerHeaderExtensionV1,
    LedgerHeaderHistoryEntry, LedgerHeaderHistoryEntryExt, StellarValue, StellarValueExt,
    StellarValueProposedMsValue, StellarValueSignedMsValue, TimePoint, TimePointMs, VecM,
};

fn test_context() -> ExecutionContext {
    ExecutionContext {
        protocol: ProtocolVersion(30),
        project: ProjectContext {
            root: ".".into(),
            name: "test".into(),
            project_type: ProjectType::Unknown,
            capabilities: vec![],
        },
        network: NetworkContext {
            name: NetworkName::Testnet,
            rpc_url: "https://example.invalid".into(),
            passphrase: "Test SDF Network ; September 2015".into(),
            observed_protocol: None,
        },
        fixtures: FixtureStore::default(),
        git: GitContext::default(),
        cache: CacheStore::new(std::env::temp_dir().join("canary-xdr-ledger-tests-cache")),
        options: RunOptions::default(),
    }
}

fn sample_ledger_header(version: u32, seq: u32, scp_ext: StellarValueExt) -> LedgerHeader {
    LedgerHeader {
        ledger_version: version,
        previous_ledger_hash: Hash([1; 32]),
        scp_value: StellarValue {
            tx_set_hash: Hash([2; 32]),
            close_time: TimePoint(1_735_689_600),
            upgrades: VecM::default(),
            ext: scp_ext,
        },
        tx_set_result_hash: Hash([3; 32]),
        bucket_list_hash: Hash([4; 32]),
        ledger_seq: seq,
        total_coins: 100_000_000_000_000,
        fee_pool: 50_000,
        inflation_seq: 10,
        id_pool: 1000,
        base_fee: 100,
        base_reserve: 10_000_000,
        max_tx_set_size: 1000,
        skip_list: [Hash([5; 32]), Hash([6; 32]), Hash([7; 32]), Hash([8; 32])],
        ext: LedgerHeaderExt::V1(LedgerHeaderExtensionV1 {
            flags: 1,
            ext: stellar_xdr::LedgerHeaderExtensionV1Ext::V0,
        }),
    }
}

fn history_entry(header: LedgerHeader) -> LedgerHeaderHistoryEntry {
    LedgerHeaderHistoryEntry {
        hash: Hash([42; 32]),
        header,
        ext: LedgerHeaderHistoryEntryExt::V0,
    }
}

#[test]
fn type_names_parse_and_display() {
    assert_eq!(
        "LedgerHeader".parse::<XdrTypeName>().unwrap(),
        XdrTypeName::LedgerHeader
    );
    assert_eq!(XdrTypeName::LedgerHeader.to_string(), "LedgerHeader");

    assert_eq!(
        "LedgerCloseMeta".parse::<XdrTypeName>().unwrap(),
        XdrTypeName::LedgerCloseMeta
    );
    assert_eq!(XdrTypeName::LedgerCloseMeta.to_string(), "LedgerCloseMeta");
}

#[test]
fn ledger_header_roundtrip_default_and_populated() {
    for header in [
        LedgerHeader::default(),
        sample_ledger_header(29, 12345, StellarValueExt::Basic),
    ] {
        let typed_base64 = encode_ledger_header(&header).expect("encodes typed");
        let generic_base64 =
            encode(&DecodedValue::LedgerHeader(Box::new(header.clone()))).expect("encodes generic");
        assert_eq!(typed_base64, generic_base64);

        let decoded_typed =
            decode_ledger_header(&typed_base64).expect("decodes typed successfully");
        assert_eq!(decoded_typed, header);

        let decoded_generic =
            decode(XdrTypeName::LedgerHeader, &typed_base64).expect("decodes generic");
        assert_eq!(decoded_generic.type_name(), XdrTypeName::LedgerHeader);

        match decoded_generic {
            DecodedValue::LedgerHeader(ref h) => assert_eq!(**h, header),
            other => panic!("expected DecodedValue::LedgerHeader, got {other:?}"),
        }

        let re_encoded = encode(&decoded_generic).expect("re-encodes generic");
        assert_eq!(re_encoded, typed_base64);
    }
}

#[test]
fn ledger_close_meta_roundtrip_v0_v1_v2() {
    let header = sample_ledger_header(30, 99999, StellarValueExt::Basic);
    let v0 = LedgerCloseMeta::V0(LedgerCloseMetaV0 {
        ledger_header: history_entry(header.clone()),
        ..Default::default()
    });
    let v1 = LedgerCloseMeta::V1(LedgerCloseMetaV1 {
        ledger_header: history_entry(header.clone()),
        ..Default::default()
    });
    let v2 = LedgerCloseMeta::V2(LedgerCloseMetaV2 {
        ledger_header: history_entry(header),
        ..Default::default()
    });

    for meta in [v0, v1, v2] {
        let typed_base64 = encode_ledger_close_meta(&meta).expect("encodes typed");
        let generic_base64 = encode(&DecodedValue::LedgerCloseMeta(Box::new(meta.clone())))
            .expect("encodes generic");
        assert_eq!(typed_base64, generic_base64);

        let decoded_typed =
            decode_ledger_close_meta(&typed_base64).expect("decodes typed successfully");
        assert_eq!(decoded_typed, meta);

        let decoded_generic =
            decode(XdrTypeName::LedgerCloseMeta, &typed_base64).expect("decodes generic");
        assert_eq!(decoded_generic.type_name(), XdrTypeName::LedgerCloseMeta);

        match decoded_generic {
            DecodedValue::LedgerCloseMeta(ref m) => assert_eq!(**m, meta),
            other => panic!("expected DecodedValue::LedgerCloseMeta, got {other:?}"),
        }

        let re_encoded = encode(&decoded_generic).expect("re-encodes generic");
        assert_eq!(re_encoded, typed_base64);
    }
}

#[test]
fn protocol_30_signed_ms_variant_in_ledger_header_and_close_meta() {
    let signed_ms_ext = StellarValueExt::SignedMs(StellarValueSignedMsValue {
        close_time_ms: TimePointMs(1_735_689_600_123),
        lc_value_signature: LedgerCloseValueSignature::default(),
    });
    let header = sample_ledger_header(30, 500_000, signed_ms_ext);

    let header_b64 = encode_ledger_header(&header).expect("encodes header");
    let decoded_header = decode_ledger_header(&header_b64).expect("decodes header");
    assert_eq!(decoded_header, header);

    match &decoded_header.scp_value.ext {
        StellarValueExt::SignedMs(val) => {
            assert_eq!(val.close_time_ms, TimePointMs(1_735_689_600_123));
        }
        other => panic!("expected SignedMs variant, got {other:?}"),
    }

    // Explicit JSON assertion verifying millisecond value for standalone LedgerHeader containing SignedMs
    let json_val = DecodedValue::LedgerHeader(Box::new(header.clone()))
        .to_json()
        .expect("converts to JSON");
    assert_eq!(
        json_val["scp_value"]["ext"]["signed_ms"]["close_time_ms"],
        "1735689600123"
    );

    let meta = LedgerCloseMeta::V2(LedgerCloseMetaV2 {
        ledger_header: history_entry(header),
        ..Default::default()
    });

    let meta_b64 = encode_ledger_close_meta(&meta).expect("encodes meta");
    let decoded_meta = decode_ledger_close_meta(&meta_b64).expect("decodes meta");
    assert_eq!(decoded_meta, meta);

    // Explicit JSON assertion verifying millisecond value for LedgerCloseMeta containing LedgerHeader with SignedMs
    let meta_json = DecodedValue::LedgerCloseMeta(Box::new(meta))
        .to_json()
        .expect("meta JSON");
    assert_eq!(
        meta_json["v2"]["ledger_header"]["header"]["scp_value"]["ext"]["signed_ms"]
            ["close_time_ms"],
        "1735689600123"
    );
}

#[test]
fn protocol_30_empty_tx_set_ms_variant_in_ledger_header_and_close_meta() {
    let empty_tx_set_ms_ext = StellarValueExt::EmptyTxSetMs(StellarValueProposedMsValue {
        close_time_ms: TimePointMs(1_735_689_600_999),
        tx_set_hash: Hash([99; 32]),
        previous_ledger_hash: Hash([1; 32]),
        previous_ledger_version: 30,
        lc_value_signature: LedgerCloseValueSignature::default(),
    });
    let header = sample_ledger_header(30, 500_001, empty_tx_set_ms_ext);

    let header_b64 = encode_ledger_header(&header).expect("encodes header");
    let decoded_header = decode_ledger_header(&header_b64).expect("decodes header");
    assert_eq!(decoded_header, header);

    match &decoded_header.scp_value.ext {
        StellarValueExt::EmptyTxSetMs(val) => {
            assert_eq!(val.close_time_ms, TimePointMs(1_735_689_600_999));
            assert_eq!(val.previous_ledger_version, 30);
            assert_eq!(val.tx_set_hash, Hash([99; 32]));
        }
        other => panic!("expected EmptyTxSetMs variant, got {other:?}"),
    }

    // Explicit JSON assertion verifying millisecond value for standalone LedgerHeader containing EmptyTxSetMs
    let json_val = DecodedValue::LedgerHeader(Box::new(header.clone()))
        .to_json()
        .expect("converts to JSON");
    assert_eq!(
        json_val["scp_value"]["ext"]["empty_tx_set_ms"]["close_time_ms"],
        "1735689600999"
    );
    assert_eq!(
        json_val["scp_value"]["ext"]["empty_tx_set_ms"]["previous_ledger_version"],
        30
    );

    let meta_v1 = LedgerCloseMeta::V1(LedgerCloseMetaV1 {
        ledger_header: history_entry(header),
        ..Default::default()
    });
    let meta_b64 = encode_ledger_close_meta(&meta_v1).expect("encodes meta v1");
    let decoded_meta = decode_ledger_close_meta(&meta_b64).expect("decodes meta v1");
    assert_eq!(decoded_meta, meta_v1);

    // Explicit JSON assertion verifying millisecond value for LedgerCloseMeta containing LedgerHeader with EmptyTxSetMs
    let meta_json = DecodedValue::LedgerCloseMeta(Box::new(meta_v1))
        .to_json()
        .expect("meta v1 JSON");
    assert_eq!(
        meta_json["v1"]["ledger_header"]["header"]["scp_value"]["ext"]["empty_tx_set_ms"]
            ["close_time_ms"],
        "1735689600999"
    );
}

#[test]
fn json_serialization_matches_expected_structure() {
    let header = sample_ledger_header(30, 100, StellarValueExt::Basic);
    let decoded = DecodedValue::LedgerHeader(Box::new(header));
    let json_value = decoded.to_json().expect("to_json succeeds");

    assert_eq!(json_value["ledger_version"], 30);
    assert_eq!(json_value["ledger_seq"], 100);
    assert_eq!(json_value["total_coins"], "100000000000000");
    assert_eq!(json_value["fee_pool"], "50000");
    assert_eq!(json_value["id_pool"], "1000");
    assert_eq!(json_value["base_fee"], 100);
    assert_eq!(json_value["base_reserve"], 10_000_000);
    assert_eq!(json_value["max_tx_set_size"], 1000);
    assert_eq!(json_value["skip_list"].as_array().unwrap().len(), 4);
    assert_eq!(json_value["ext"]["v1"]["flags"], 1);
}

#[test]
fn typed_error_reporting_for_invalid_truncated_and_trailing() {
    let header_b64 = encode_ledger_header(&sample_ledger_header(30, 1, StellarValueExt::Basic))
        .expect("encodes");
    let header_bytes = STANDARD.decode(&header_b64).expect("decodes b64");

    // Invalid base64
    let err = decode_ledger_header("not-valid-base64!!!").unwrap_err();
    assert_eq!(err.byte_offset(), 0);
    assert!(matches!(err, XdrDecodeError::InvalidBase64(_)));

    // Truncated buffer
    let truncated = &header_bytes[..header_bytes.len() - 10];
    let truncated_b64 = STANDARD.encode(truncated);
    let err = decode_ledger_header(&truncated_b64).unwrap_err();
    assert!(err.byte_offset() > 0 && err.byte_offset() <= truncated.len() as u64);
    match err {
        XdrDecodeError::InvalidXdr {
            type_name, offset, ..
        } => {
            assert_eq!(type_name, XdrTypeName::LedgerHeader);
            assert!(offset > 0);
        }
        other => panic!("expected InvalidXdr, got {other:?}"),
    }

    // Trailing bytes
    let mut trailing = header_bytes.clone();
    trailing.extend_from_slice(&[0, 0, 0, 0]);
    let trailing_b64 = STANDARD.encode(&trailing);
    let err = decode_ledger_header(&trailing_b64).unwrap_err();
    assert_eq!(err.byte_offset(), header_bytes.len() as u64);

    // Same for LedgerCloseMeta
    let meta_b64 = encode_ledger_close_meta(&LedgerCloseMeta::default()).expect("encodes meta");
    let meta_bytes = STANDARD.decode(&meta_b64).expect("decodes b64");

    let err = decode_ledger_close_meta("bad base64!!").unwrap_err();
    assert_eq!(err.byte_offset(), 0);

    let truncated_meta = &meta_bytes[..meta_bytes.len() - 4];
    let truncated_meta_b64 = STANDARD.encode(truncated_meta);
    let err = decode_ledger_close_meta(&truncated_meta_b64).unwrap_err();
    assert!(err.byte_offset() > 0 && err.byte_offset() <= truncated_meta.len() as u64);

    let mut trailing_meta = meta_bytes.clone();
    trailing_meta.extend_from_slice(&[1, 2, 3, 4]);
    let trailing_meta_b64 = STANDARD.encode(&trailing_meta);
    let err = decode_ledger_close_meta(&trailing_meta_b64).unwrap_err();
    assert_eq!(err.byte_offset(), meta_bytes.len() as u64);

    // Trailing bytes on generic decode path for both types
    let generic_header_err = decode(XdrTypeName::LedgerHeader, &trailing_b64);
    assert!(
        generic_header_err.is_err(),
        "generic decode must reject trailing bytes for LedgerHeader"
    );
    let generic_meta_err = decode(XdrTypeName::LedgerCloseMeta, &trailing_meta_b64);
    assert!(
        generic_meta_err.is_err(),
        "generic decode must reject trailing bytes for LedgerCloseMeta"
    );
}

#[test]
fn fixture_runner_compatibility_all_assertions() {
    let header = sample_ledger_header(30, 42, StellarValueExt::Basic);
    let header_b64 = encode_ledger_header(&header).expect("encodes header");

    // 1. roundtrip assertion on LedgerHeader
    let roundtrip_body = format!(
        "id = \"p30-header-roundtrip\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"header roundtrip\"\ntype = \"LedgerHeader\"\nkind = \"roundtrip\"\nvalue_base64 = \"{header_b64}\"\n"
    );
    let fixture = XdrFixture::from_loaded(
        &parse_fixture_str(&roundtrip_body, Path::new("test.toml")).unwrap(),
    )
    .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "LedgerHeader round-tripped byte-for-byte");

    // 2. decode-success assertion on LedgerHeader
    let success_body = format!(
        "id = \"p30-header-success\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"header success\"\ntype = \"LedgerHeader\"\nkind = \"decode-success\"\nvalue_base64 = \"{header_b64}\"\n"
    );
    let fixture =
        XdrFixture::from_loaded(&parse_fixture_str(&success_body, Path::new("test.toml")).unwrap())
            .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);

    // 3. decode-failure assertion on malformed XDR input (valid base64 that fails XDR decoding)
    let fail_body =
        "id = \"p30-header-fail\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"header fail\"\ntype = \"LedgerHeader\"\nkind = \"decode-failure\"\nvalue_base64 = \"AAAA\"\n";
    let fixture =
        XdrFixture::from_loaded(&parse_fixture_str(fail_body, Path::new("test.toml")).unwrap())
            .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "LedgerHeader was correctly rejected");

    // 4. encode-equals assertion
    let encode_equals_body = format!(
        "id = \"p30-header-eq\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"header eq\"\ntype = \"LedgerHeader\"\nkind = \"encode-equals\"\nvalue_base64 = \"{header_b64}\"\nexpected_base64 = \"{header_b64}\"\n"
    );
    let fixture = XdrFixture::from_loaded(
        &parse_fixture_str(&encode_equals_body, Path::new("test.toml")).unwrap(),
    )
    .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);

    // 5. decode-json-equals assertion
    let decoded_header =
        decode(XdrTypeName::LedgerHeader, &header_b64).expect("decodes for json test");
    let json_text = serde_json::to_string_pretty(&decoded_header.to_json().unwrap()).unwrap();

    let json_equals_body = format!(
        "id = \"p30-header-json\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"header json\"\ntype = \"LedgerHeader\"\nkind = \"decode-json-equals\"\nvalue_base64 = \"{header_b64}\"\nexpected_json = '''\n{json_text}\n'''\n"
    );
    let fixture = XdrFixture::from_loaded(
        &parse_fixture_str(&json_equals_body, Path::new("test.toml")).unwrap(),
    )
    .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);
    assert_eq!(
        result.summary,
        "decoded LedgerHeader matched the expected JSON"
    );

    // 6. LedgerCloseMeta fixture assertions across all kinds
    let meta = LedgerCloseMeta::V1(LedgerCloseMetaV1 {
        ledger_header: history_entry(header),
        ..Default::default()
    });
    let meta_b64 = encode_ledger_close_meta(&meta).expect("encodes meta");

    // 6a. roundtrip on LedgerCloseMeta
    let meta_roundtrip_body = format!(
        "id = \"p30-meta-roundtrip\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"meta roundtrip\"\ntype = \"LedgerCloseMeta\"\nkind = \"roundtrip\"\nvalue_base64 = \"{meta_b64}\"\n"
    );
    let fixture = XdrFixture::from_loaded(
        &parse_fixture_str(&meta_roundtrip_body, Path::new("test.toml")).unwrap(),
    )
    .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);
    assert_eq!(
        result.summary,
        "LedgerCloseMeta round-tripped byte-for-byte"
    );

    // 6b. decode-success on LedgerCloseMeta
    let meta_success_body = format!(
        "id = \"p30-meta-success\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"meta success\"\ntype = \"LedgerCloseMeta\"\nkind = \"decode-success\"\nvalue_base64 = \"{meta_b64}\"\n"
    );
    let fixture = XdrFixture::from_loaded(
        &parse_fixture_str(&meta_success_body, Path::new("test.toml")).unwrap(),
    )
    .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "decoded LedgerCloseMeta successfully");

    // 6c. decode-failure on LedgerCloseMeta (valid base64 that is malformed XDR)
    let meta_fail_body =
        "id = \"p30-meta-fail\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"meta fail\"\ntype = \"LedgerCloseMeta\"\nkind = \"decode-failure\"\nvalue_base64 = \"AAAA\"\n";
    let fixture = XdrFixture::from_loaded(
        &parse_fixture_str(meta_fail_body, Path::new("test.toml")).unwrap(),
    )
    .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "LedgerCloseMeta was correctly rejected");

    // 6d. encode-equals on LedgerCloseMeta
    let meta_encode_eq_body = format!(
        "id = \"p30-meta-eq\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"meta eq\"\ntype = \"LedgerCloseMeta\"\nkind = \"encode-equals\"\nvalue_base64 = \"{meta_b64}\"\nexpected_base64 = \"{meta_b64}\"\n"
    );
    let fixture = XdrFixture::from_loaded(
        &parse_fixture_str(&meta_encode_eq_body, Path::new("test.toml")).unwrap(),
    )
    .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);
    assert_eq!(
        result.summary,
        "LedgerCloseMeta encoded to the expected bytes"
    );

    // 6e. decode-json-equals on LedgerCloseMeta
    let meta_decoded = decode(XdrTypeName::LedgerCloseMeta, &meta_b64).expect("decodes meta");
    let meta_json_text = serde_json::to_string_pretty(&meta_decoded.to_json().unwrap()).unwrap();

    let meta_json_body = format!(
        "id = \"p30-meta-json\"\nprotocol = 30\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"meta json\"\ntype = \"LedgerCloseMeta\"\nkind = \"decode-json-equals\"\nvalue_base64 = \"{meta_b64}\"\nexpected_json = '''\n{meta_json_text}\n'''\n"
    );
    let fixture = XdrFixture::from_loaded(
        &parse_fixture_str(&meta_json_body, Path::new("test.toml")).unwrap(),
    )
    .unwrap();
    let result = DefaultXdrRunner.run(&fixture, &test_context()).unwrap();
    assert_eq!(result.status, Status::Pass);
    assert_eq!(
        result.summary,
        "decoded LedgerCloseMeta matched the expected JSON"
    );
}
