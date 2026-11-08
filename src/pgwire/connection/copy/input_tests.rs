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
fn should_preserve_text_copy_escape_and_final_record_behavior() {
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
fn should_stop_binary_column_lookup_when_copy_is_cancelled() {
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
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_decode_binary_text_and_bytea_with_admitted_output() {
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
fn should_preserve_binary_utf8_at_chunk_boundary_and_reject_truncated_sequence() {
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
fn should_release_binary_output_when_cancelled_during_conversion() {
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
        Err(crate::app::CassieError::QueryCancelled)
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
fn should_preserve_fixed_width_short_and_excess_errors() {
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
fn should_enforce_binary_fixed_text_overlap_budget_and_cancellation() {
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
    let cancelled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::binary_fixed::decode(16, &[1], 0, &cancelled),
        Err(crate::app::CassieError::QueryCancelled)
    ));
}

#[test]
fn should_match_uuid_binary_text_and_length_errors() {
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
fn should_match_name_binary_width_utf8_and_nul_behavior() {
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
fn should_admit_name_binary_at_exact_budget_and_honor_cancellation() {
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
        Err(crate::app::CassieError::QueryCancelled)
    ));
}
