use super::parse_text_payload;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

#[test]
fn should_preserve_text_copy_fields_while_holding_decoder_admission() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let payload = b"\\N\ta\\tb\r\n\\\\N\tlast";
    // Act
    let (rows, memory) = parse_text_payload(payload, '\t', "\\N", &controls).expect("parse");
    // Assert
    assert_eq!(
        rows,
        vec![
            vec![None, Some("a\tb".into())],
            vec![Some("\\N".into()), Some("last".into())]
        ]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_text_copy_row_directory_before_allocating_over_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = parse_text_payload(b"one\ttwo\n", '\t', "\\N", &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_text_copy_escape_with_final_record_behavior() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let payload = b"\\101\t\\x41\tend\\";
    // Act
    let (rows, memory) = parse_text_payload(payload, '\t', "\\N", &controls).expect("parse");
    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Some("101".into()),
            Some("x41".into()),
            Some("end\\".into())
        ]]
    );
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_binary_row_directory_admission_through_output() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE copy_binary_budget (id INT)", vec![])
        .expect("table");
    let parsed = crate::sql::parser::parse_statement("COPY copy_binary_budget (id) FROM STDIN")
        .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&4_i32.to_be_bytes());
    payload.extend_from_slice(&42_i32.to_be_bytes());
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    let (rows, memory) =
        super::parse_binary_payload(&payload, &cassie, &statement, &controls).expect("binary");
    // Assert
    assert_eq!(rows, vec![vec![Some("42".into())]]);
    assert!(controls.current_query_memory_bytes() > 0);
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_binary_row_directory_when_budget_is_exhausted() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE copy_binary_small (id INT)", vec![])
        .expect("table");
    let parsed = crate::sql::parser::parse_statement("COPY copy_binary_small (id) FROM STDIN")
        .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&(-1_i32).to_be_bytes());
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: std::mem::size_of::<i64>(),
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = super::parse_binary_payload(&payload, &cassie, &statement, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_stop_binary_column_lookup_when_copy_is_canceled() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let parsed = crate::sql::parser::parse_statement("COPY t (id) FROM STDIN").expect("COPY");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    cancellation.cancel();
    // Act
    let result = super::copy_column_types(&statement, &[], &controls);
    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCanceled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_decode_binary_text_with_bytea_with_admitted_output() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    for (oid, bytes, expected) in [
        (25, "é\0".as_bytes(), "é\0"),
        (1042, b"".as_slice(), ""),
        (1043, b"alpha".as_slice(), "alpha"),
        (17, &[0, 0x7f, 0x80, 0xff][..], "\\x007f80ff"),
    ] {
        // Act
        let (text, memory) = super::binary_text::decode(oid, bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(text, expected);
        assert_eq!(controls.current_query_memory_bytes(), text.capacity());
        drop((text, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_invalid_binary_text_codec_error() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let bytes = &[0xff];
    for oid in [25, 1042, 1043] {
        let original =
            super::super::super::codecs::binary_to_value(bytes, oid).expect_err("invalid UTF-8");
        // Act
        let result = super::binary_text::decode(oid, bytes, 2, &controls);
        // Assert
        assert!(
            matches!(result, Err(crate::app::CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_deny_binary_bytea_output_while_input_uses_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 9,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(4).expect("input");
    // Act
    let result = super::binary_text::decode(17, &[0, 0x7f], 0, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 4);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_binary_utf8_at_chunk_boundary_with_reject_truncated_sequence() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut bytes = vec![b'a'; 4095];
    bytes.extend_from_slice("é".as_bytes());
    // Act
    let (text, memory) = super::binary_text::decode(25, &bytes, 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(text.as_bytes(), bytes);
    drop((text, memory));
    bytes.pop();
    assert!(matches!(
        super::binary_text::decode(25, &bytes, 0, &controls),
        Err(crate::app::CassieError::Parse(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_binary_output_when_canceled_during_conversion() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    // Act
    let result =
        super::binary_text::decode_observed(17, &[0, 1], 0, &controls, || cancellation.cancel());
    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCanceled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_binary_bytea_output_at_exact_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 10,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(4).expect("input");
    // Act
    let (output, memory) = super::binary_text::decode(17, &[0, 0x7f], 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(output, "\\x007f");
    assert_eq!(controls.current_query_memory_bytes(), 10);
    drop((output, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_existing_binary_fixed_width_outputs() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let cases = [
        (16, vec![0]),
        (16, vec![1]),
        (16, vec![255]),
        (21, i16::MIN.to_be_bytes().to_vec()),
        (21, i16::MAX.to_be_bytes().to_vec()),
        (23, i32::MIN.to_be_bytes().to_vec()),
        (23, i32::MAX.to_be_bytes().to_vec()),
        (20, i64::MIN.to_be_bytes().to_vec()),
        (20, i64::MAX.to_be_bytes().to_vec()),
        (26, u32::MAX.to_be_bytes().to_vec()),
    ];
    for (oid, bytes) in cases {
        let value = super::super::super::codecs::binary_to_value(&bytes, oid).expect("old codec");
        let expected = super::super::super::codecs::value_to_text(&value, oid);
        // Act
        let (text, memory) = super::binary_fixed::decode(oid, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(text, expected);
        drop((text, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_fixed_width_short_with_excess_errors() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    for (oid, bytes) in [
        (16, vec![]),
        (16, vec![1, 0]),
        (21, vec![0]),
        (21, vec![0, 0, 0]),
        (23, vec![0; 3]),
        (23, vec![0; 5]),
        (20, vec![0; 7]),
        (20, vec![0; 9]),
        (26, vec![0; 3]),
        (26, vec![0; 5]),
    ] {
        let original =
            super::super::super::codecs::binary_to_value(&bytes, oid).expect_err("invalid width");
        // Act
        let result = super::binary_fixed::decode(oid, &bytes, 2, &controls);
        // Assert
        assert!(
            matches!(result, Err(crate::app::CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_binary_fixed_text_overlap_budget_with_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 21,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(2).expect("input");
    // Act
    let denied = super::binary_fixed::decode(20, &i64::MIN.to_be_bytes(), 0, &controls);
    // Assert
    assert!(denied.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 2);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::binary_fixed::decode(16, &[1], 0, &canceled),
        Err(crate::app::CassieError::QueryCanceled)
    ));
}

#[test]
fn should_match_uuid_binary_text_with_length_errors() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    for bytes in [vec![0; 16], vec![0xff; 16], (0_u8..16).collect()] {
        let original =
            super::super::super::codecs::binary_to_value(&bytes, 2950).expect("original UUID");
        let expected = super::super::super::codecs::value_to_text(&original, 2950);
        // Act
        let (text, memory) = super::binary_uuid::decode(2950, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(text, expected);
        drop((text, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    for length in (0..16).chain([17, 64]) {
        let bytes = vec![0; length];
        let original =
            super::super::super::codecs::binary_to_value(&bytes, 2950).expect_err("invalid width");
        let decoded = super::binary_uuid::decode(2950, &bytes, 2, &controls);
        assert!(
            matches!(decoded, Err(crate::app::CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_uuid_output_at_exact_input_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 52,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(16).expect("input");
    // Act
    let (text, memory) = super::binary_uuid::decode(2950, &[0; 16], 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(text, "00000000-0000-0000-0000-000000000000");
    assert_eq!(controls.current_query_memory_bytes(), 52);
    drop((text, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let denied_limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 51,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&denied_limits, Instant::now());
    let input = denied.reserve_query_memory(16).expect("input");
    assert!(super::binary_uuid::decode(2950, &[0; 16], 0, &denied).is_err());
    drop(input);
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_name_binary_width_utf8_with_nul_behavior() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut cases = vec![
        vec![],
        b"name".to_vec(),
        b"a\0b".to_vec(),
        "é".as_bytes().to_vec(),
        vec![b'a'; 63],
        vec![b'a'; 64],
        vec![b'a'; 80],
        vec![0xff],
    ];
    cases.push(vec![0xc3]);
    cases.push(format!("{}é", "a".repeat(61)).into_bytes());
    cases.push(format!("{}é", "a".repeat(62)).into_bytes());
    let mut overlong_invalid = vec![b'a'; 64];
    overlong_invalid[0] = 0xff;
    cases.push(overlong_invalid);
    for bytes in cases {
        let original = super::super::super::codecs::binary_to_value(&bytes, 19);
        // Act
        let direct = super::binary_name::decode(19, &bytes, 2, &controls);
        // Assert
        match original {
            Ok(value) => {
                let expected = super::super::super::codecs::value_to_text(&value, 19);
                let (text, memory) = direct.expect("decode").expect("handled");
                assert_eq!(text, expected);
                drop((text, memory));
            }
            Err(error) => assert!(
                matches!(direct, Err(crate::app::CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {error}"))
            ),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_name_binary_input_output_budget_overlap() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 7,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(4).expect("input");
    // Act
    let result = super::binary_name::decode(19, b"name", 0, &controls);
    // Assert
    assert!(result.is_err());
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_name_binary_at_exact_budget_with_honor_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 8,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(4).expect("input");
    // Act
    let (text, memory) = super::binary_name::decode(19, b"name", 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(text, "name");
    assert_eq!(controls.current_query_memory_bytes(), 8);
    drop((text, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::binary_name::decode(19, b"name", 0, &controls),
        Err(crate::app::CassieError::QueryCanceled)
    ));
}

#[test]
fn should_match_binary_mac_with_lsn_text_with_width_errors() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let cases = [
        (829, vec![0; 6]),
        (829, vec![0xff; 6]),
        (829, vec![0, 1, 2, 127, 128, 255]),
        (774, vec![0; 8]),
        (774, vec![0xff; 8]),
        (774, (0_u8..8).collect()),
        (3220, 0_u64.to_be_bytes().to_vec()),
        (3220, 1_u64.to_be_bytes().to_vec()),
        (3220, (1_u64 << 63).to_be_bytes().to_vec()),
        (3220, u64::MAX.to_be_bytes().to_vec()),
    ];
    for (oid, bytes) in cases {
        let original = super::super::super::codecs::binary_to_value(&bytes, oid).expect("original");
        let expected = super::super::super::codecs::value_to_text(&original, oid);
        // Act
        let (text, memory) = super::binary_network::decode(oid, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(text, expected);
        drop((text, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    for (oid, width) in [(829, 6_usize), (774, 8), (3220, 8)] {
        for length in (0..width).chain([width + 1, width + 16]) {
            let bytes = vec![0; length];
            let original =
                super::super::super::codecs::binary_to_value(&bytes, oid).expect_err("width");
            let result = super::binary_network::decode(oid, &bytes, 2, &controls);
            assert!(
                matches!(result, Err(crate::app::CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
            );
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_enforce_binary_mac_output_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 23,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(6).expect("input");
    // Act
    let (text, memory) = super::binary_network::decode(829, &[0; 6], 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(text, "00:00:00:00:00:00");
    assert_eq!(controls.current_query_memory_bytes(), 23);
    drop((text, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_binary_mac_output_below_overlap_budget_with_release() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 22,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(6).expect("input");
    // Act
    let result = super::binary_network::decode(829, &[0; 6], 0, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 6);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn network_frame(family: u8, bits: u8, address: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        family,
        bits,
        0,
        u8::try_from(address.len()).expect("address length"),
    ];
    bytes.extend_from_slice(address);
    bytes
}

#[test]
fn should_match_binary_inet_with_cidr_formatting_with_errors() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let cases = [
        (869, network_frame(2, 32, &[192, 0, 2, 1])),
        (869, network_frame(2, 24, &[192, 0, 2, 1])),
        (650, network_frame(2, 24, &[192, 0, 2, 0])),
        (869, network_frame(3, 128, &[0; 16])),
        (869, network_frame(2, 0, &[192, 0, 2, 1])),
        (650, network_frame(2, 0, &[0, 0, 0, 0])),
        (650, network_frame(3, 0, &[0; 16])),
        (650, network_frame(3, 128, &[0; 16])),
        (650, network_frame(3, 128, &[255; 16])),
        (
            869,
            network_frame(3, 128, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 192, 0, 2, 1]),
        ),
        (
            869,
            network_frame(
                3,
                128,
                &[0x20, 1, 0, 0, 0, 0, 0x20, 2, 0, 0, 0, 0, 0x20, 3, 0x20, 4],
            ),
        ),
        (
            869,
            network_frame(3, 128, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
        ),
        (
            869,
            network_frame(
                3,
                128,
                &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 192, 0, 2, 1],
            ),
        ),
        (
            869,
            network_frame(
                3,
                64,
                &[0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            ),
        ),
        (
            650,
            network_frame(
                3,
                64,
                &[0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            ),
        ),
    ];
    for (oid, bytes) in cases {
        let original = super::super::super::codecs::binary_to_value(&bytes, oid).expect("original");
        let expected = super::super::super::codecs::value_to_text(&original, oid);
        // Act
        let (text, memory) = super::binary_inet::decode(oid, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(text, expected);
        drop((text, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    for (oid, bytes) in [
        (869, vec![]),
        (869, vec![9, 0, 0, 4, 0, 0, 0, 0]),
        (869, vec![2, 33, 0, 4, 0, 0, 0, 0]),
        (869, vec![2, 32, 0, 4, 0, 0, 0]),
        (869, vec![2, 32, 0, 4, 0, 0, 0, 0, 1]),
        (650, vec![2, 24, 0, 4, 192, 0, 2, 1]),
    ] {
        let original =
            super::super::super::codecs::binary_to_value(&bytes, oid).expect_err("invalid");
        let result = super::binary_inet::decode(oid, &bytes, 2, &controls);
        assert!(
            matches!(result, Err(crate::app::CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_binary_inet_output_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 17,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    let bytes = network_frame(2, 32, &[192, 0, 2, 1]);
    // Act
    let (text, memory) = super::binary_inet::decode(869, &bytes, 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(text, "192.0.2.1");
    assert_eq!(controls.current_query_memory_bytes(), 17);
    drop((text, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_binary_inet_below_overlap_budget_with_honor_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 16,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    let bytes = network_frame(2, 32, &[192, 0, 2, 1]);
    // Act
    let result = super::binary_inet::decode(869, &bytes, 0, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 8);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::binary_inet::decode(869, &bytes, 0, &canceled),
        Err(crate::app::CassieError::QueryCanceled)
    ));
}

#[test]
fn should_admit_maximum_binary_cidr_spelling_at_exact_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 63,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let bytes = network_frame(3, 128, &[255; 16]);
    let input = controls.reserve_query_memory(bytes.len()).expect("input");
    // Act
    let (text, memory) = super::binary_inet::decode(650, &bytes, 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(text, "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/128");
    assert_eq!(controls.current_query_memory_bytes(), 63);
    drop((text, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_binary_real_text_with_error_order() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = [
        0.0_f32,
        -0.0,
        0.1,
        -0.1,
        1e-5,
        1e-4,
        f32::from_bits(1e-4_f32.to_bits() - 1),
        f32::from_bits(1e-4_f32.to_bits() + 1),
        1e5,
        1e6,
        f32::from_bits(1e6_f32.to_bits() - 1),
        f32::from_bits(1e6_f32.to_bits() + 1),
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::MAX,
        f32::MIN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    // Act
    for value in values {
        let bytes = value.to_be_bytes();
        let original = super::super::super::codecs::binary_to_value(&bytes, 700).expect("original");
        let expected = super::super::super::codecs::value_to_text(&original, 700);
        let (actual, memory) = super::binary_real::decode(700, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(actual, expected, "bits {:08x}", value.to_bits());
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    for bytes in [
        vec![],
        vec![0; 1],
        vec![0; 2],
        vec![0; 3],
        vec![0; 5],
        vec![0; 64],
    ] {
        let original =
            super::super::super::codecs::binary_to_value(&bytes, 700).expect_err("invalid");
        let result = super::binary_real::decode(700, &bytes, 2, &controls);
        assert!(
            matches!(result, Err(crate::app::CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
        );
    }
}

#[test]
fn should_enforce_binary_real_input_output_overlap_with_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 7,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(4).expect("input");
    // Act
    let (text, memory) = super::binary_real::decode(700, &0.1_f32.to_be_bytes(), 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(text, "0.1");
    assert_eq!(controls.current_query_memory_bytes(), 7);
    drop((text, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::binary_real::decode(700, &0.1_f32.to_be_bytes(), 0, &canceled),
        Err(crate::app::CassieError::QueryCanceled)
    ));
}

#[test]
fn should_deny_binary_real_below_output_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 6,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(4).expect("input");
    // Act
    let result = super::binary_real::decode(700, &0.1_f32.to_be_bytes(), 0, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 4);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_binary_float8_formatting_with_error_order() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = [
        0.0_f64,
        -0.0,
        0.1,
        -0.1,
        1e-4,
        f64::from_bits(1e-4_f64.to_bits() - 1),
        1e15,
        f64::from_bits(1e15_f64.to_bits() - 1),
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::MIN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::from_bits(1.0_f64.to_bits() - 1),
        f64::from_bits(1.0_f64.to_bits() + 1),
    ];
    // Act
    for value in values {
        let bytes = value.to_be_bytes();
        let original = super::super::super::codecs::binary_to_value(&bytes, 701).expect("original");
        let expected = super::super::super::codecs::value_to_text(&original, 701);
        let (actual, memory) = super::binary_float8::decode(701, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(actual, expected, "bits {:016x}", value.to_bits());
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    for length in [0, 1, 7, 9, 64] {
        let bytes = vec![0; length];
        let original =
            super::super::super::codecs::binary_to_value(&bytes, 701).expect_err("invalid");
        let result = super::binary_float8::decode(701, &bytes, 2, &controls);
        assert!(
            matches!(result, Err(crate::app::CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
        );
    }
}

#[test]
fn should_enforce_binary_float8_input_output_budget_with_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 11,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    // Act
    let (text, memory) = super::binary_float8::decode(701, &0.1_f64.to_be_bytes(), 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(text, "0.1");
    assert_eq!(controls.current_query_memory_bytes(), 11);
    drop((text, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::binary_float8::decode(701, &0.1_f64.to_be_bytes(), 0, &canceled),
        Err(crate::app::CassieError::QueryCanceled)
    ));
}

#[test]
fn should_deny_binary_float8_below_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 10,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    // Act
    let result = super::binary_float8::decode(701, &0.1_f64.to_be_bytes(), 0, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 8);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
