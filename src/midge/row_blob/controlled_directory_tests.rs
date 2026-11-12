use super::controlled_directory::{Directory, Progress};
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn row(entries: &[(u32, u8, u32, u32)], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.push(FORMAT_VERSION);
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&8_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(0xff);
    bytes.push(0);
    bytes.extend_from_slice(&u32::try_from(entries.len()).expect("count").to_be_bytes());
    for (id, tag, offset, length) in entries {
        bytes.extend_from_slice(&id.to_be_bytes());
        bytes.push(*tag);
        bytes.extend_from_slice(&offset.to_be_bytes());
        bytes.extend_from_slice(&length.to_be_bytes());
    }
    bytes.extend_from_slice(payload);
    bytes
}

fn parse_error(row: &[u8]) -> Option<String> {
    match RowDirectory::parse(row) {
        Ok(_) => None,
        Err(error) => Some(error.to_string()),
    }
}

#[test]
fn should_match_legacy_error_priority_for_overlapping_directory_entries() {
    // Arrange
    let controls = controls();
    let cases = [
        row(&[(1, TYPE_JSON, u32::MAX, 3), (1, TYPE_JSON, 0, 2)], b"{}"),
        row(&[(1, TYPE_JSON, 0, 2), (1, TYPE_JSON, u32::MAX, 3)], b"{}"),
        row(&[(1, TYPE_JSON, 0, 1), (2, TYPE_JSON, 0, 1)], b"{}"),
        row(&[(1, TYPE_JSON, 0, 0), (2, TYPE_JSON, 0, 2)], b"{}"),
        row(&[(1, TYPE_JSON, 1, 1)], b"{}"),
        row(&[(1, TYPE_JSON, 0, 1)], b"{}"),
        row(&[(1, TYPE_JSON, 0, 3)], b"{}"),
    ];
    // Act
    let outcomes = cases
        .iter()
        .map(|bytes| {
            let actual = match Directory::parse(bytes, &controls) {
                Ok(_) => None,
                Err(error) => Some(error.to_string()),
            };
            (
                parse_error(bytes),
                actual,
                controls.current_query_memory_bytes(),
            )
        })
        .collect::<Vec<_>>();

    // Assert
    for (expected, actual, retained) in outcomes {
        assert_eq!(actual, expected);
        assert_eq!(retained, 0);
    }
}

#[test]
fn should_match_legacy_errors_for_malformed_directory_prefixes() {
    // Arrange
    let controls = controls();
    let complete = row(&[(1, TYPE_JSON, 0, 2)], b"{}");
    let mut bad_version = complete.clone();
    bad_version[4] = FORMAT_VERSION.wrapping_add(1);
    let mut bad_magic = complete.clone();
    bad_magic[0] ^= 1;
    let truncated = complete[..complete.len() - 4].to_vec();
    let cases = [Vec::new(), bad_magic, bad_version, truncated];
    // Act
    let outcomes = cases
        .iter()
        .map(|bytes| {
            let actual = match Directory::parse(bytes, &controls) {
                Ok(_) => None,
                Err(error) => Some(error.to_string()),
            };
            (
                parse_error(bytes),
                actual,
                controls.current_query_memory_bytes(),
            )
        })
        .collect::<Vec<_>>();

    // Assert
    for (expected, actual, retained) in outcomes {
        assert_eq!(actual, expected);
        assert_eq!(retained, 0);
    }
}

#[test]
fn should_match_legacy_json_span_results_for_unusual_field_encodings() {
    // Arrange
    let controls = controls();
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Jsonb,
            nullable: true,
        }],
    });
    let null_json = encode_row(&schema, &serde_json::json!({"doc":null})).expect("JSON null");
    let missing = encode_row(&schema, &serde_json::json!({})).expect("missing");
    let text_schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Text,
            nullable: true,
        }],
    });
    let sql_null = encode_row(&text_schema, &serde_json::json!({"doc":null})).expect("SQL null");
    let wrong_tag =
        encode_row(&text_schema, &serde_json::json!({"doc":"text"})).expect("wrong tag");
    let field = &schema.fields[0];
    // Act
    let spans = [
        (&null_json, Some(&b"null"[..])),
        (&missing, None),
        (&sql_null, None),
    ]
    .map(|(bytes, expected)| {
        let directory = Directory::parse(bytes, &controls).expect("directory");
        let actual = directory.json_span(field, &controls).expect("field");
        (actual == expected, actual.map(<[u8]>::to_vec))
    });
    let expected = match RowDirectory::parse(&wrong_tag)
        .expect("legacy")
        .decode(field)
    {
        Ok(_) => panic!("accepted tag"),
        Err(error) => error.to_string(),
    };
    let directory = Directory::parse(&wrong_tag, &controls).expect("directory");
    let actual = directory
        .json_span(field, &controls)
        .expect_err("wrong tag")
        .to_string();
    drop(directory);

    // Assert
    for (matched, _) in spans {
        assert!(matched);
    }
    assert_eq!(actual, expected);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_adopt_existing_jsonb_payload_into_an_owned_plain_cell() {
    // Arrange
    let controls = controls();
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Jsonb,
            nullable: true,
        }],
    });
    let value = serde_json::json!({"doc":{"$serde_json::private::Number":"7"}});
    let bytes = encode_row(&schema, &value).expect("row");
    let directory = Directory::parse(&bytes, &controls).expect("directory");
    // Act
    let cell = directory
        .json_cell(&schema.fields[0], &controls)
        .expect("handoff")
        .expect("cell");
    drop(directory);
    drop(bytes);
    // Assert
    assert_eq!(cell.text(), r#"{"$serde_json::private::Number": "7"}"#);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(cell);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_utf8_in_a_borrowed_json_field_before_handoff() {
    // Arrange
    let controls = controls();
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Json,
            nullable: true,
        }],
    });
    let invalid = row(&[(1, TYPE_JSON, 0, 3)], &[b'"', 0xff, b'"']);
    let directory = Directory::parse(&invalid, &controls).expect("directory");
    // Act
    let result = directory.json_cell(&schema.fields[0], &controls);
    // Assert
    let expected = decode_directory_value(TYPE_JSON, &[b'"', 0xff, b'"'])
        .expect_err("legacy")
        .to_string();
    assert!(
        matches!(result, Err(super::controlled_directory::HandoffError::Cell(crate::runtime::accounted::json_arena::PlainCellError::Syntax(error))) if format!("parse error: {}", error.as_str()) == expected)
    );
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_legacy_syntax_before_later_invalid_utf8() {
    // Arrange
    let controls = controls();
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Json,
            nullable: true,
        }],
    });
    let payload = [0x5b_u8, 0x7d, 0xff];
    let expected = decode_directory_value(TYPE_JSON, &payload)
        .expect_err("legacy")
        .to_string();
    let bytes = row(&[(1, TYPE_JSON, 0, 3)], &payload);
    let directory = Directory::parse(&bytes, &controls).expect("directory");
    // Act
    let result = directory.json_cell(&schema.fields[0], &controls);
    // Assert
    assert!(
        matches!(result, Err(super::controlled_directory::HandoffError::Cell(crate::runtime::accounted::json_arena::PlainCellError::Syntax(error))) if format!("parse error: {}", error.as_str()) == expected)
    );
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_plain_valid_sentinel_even_when_legacy_value_parse_fails() {
    // Arrange
    let controls = controls();
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Json,
            nullable: true,
        }],
    });
    let payload = br#"{"$serde_json::private::Number":"bad"}"#;
    // The ordinary-object contract (#809) keeps the sentinel key as plain data
    // on both the owned and borrowed routes.
    assert_eq!(
        decode_directory_value(TYPE_JSON, payload).expect("ordinary object"),
        serde_json::json!({"$serde_json::private::Number": "bad"})
    );
    let bytes = row(
        &[(
            1,
            TYPE_JSON,
            0,
            u32::try_from(payload.len()).expect("length"),
        )],
        payload,
    );
    let directory = Directory::parse(&bytes, &controls).expect("directory");
    // Act
    let cell = directory
        .json_cell(&schema.fields[0], &controls)
        .expect("plain handoff")
        .expect("cell");
    // Assert
    assert_eq!(cell.text().as_bytes(), payload);
    drop(cell);
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_multibyte_text_across_borrowed_field_copy_boundaries() {
    // Arrange
    let controls = controls();
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Json,
            nullable: true,
        }],
    });
    let document = format!("\"{}é\"", "x".repeat(1022));
    let length = u32::try_from(document.len()).expect("length");
    let bytes = row(&[(1, TYPE_JSON, 0, length)], document.as_bytes());
    let directory = Directory::parse(&bytes, &controls).expect("directory");
    // Act
    let cell = directory
        .json_cell(&schema.fields[0], &controls)
        .expect("handoff")
        .expect("cell");
    // Assert
    assert_eq!(cell.text(), document);
    drop(cell);
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_partial_cell_after_actual_borrowed_field_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Json,
            nullable: true,
        }],
    });
    let bytes = row(&[(1, TYPE_JSON, 0, 15)], br#"{"a":"payload"}"#);
    let directory = Directory::parse(&bytes, &controls).expect("directory");
    let retained = controls.current_query_memory_bytes();
    let mut copied = 0;
    // Act
    let result = directory.json_cell_observing(&schema.fields[0], &controls, || {
        copied += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(copied, 1);
    assert!(matches!(
        result,
        Err(super::controlled_directory::HandoffError::Cell(
            crate::runtime::accounted::json_arena::PlainCellError::Control(
                CassieError::QueryCanceled
            )
        ))
    ));
    assert_eq!(controls.current_query_memory_bytes(), retained);
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_foreign_tracker_for_borrowed_json_handoff() {
    // Arrange
    let first = controls();
    let second = controls();
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Json,
            nullable: true,
        }],
    });
    let bytes = row(&[(1, TYPE_JSON, 0, 2)], b"{}");
    let directory = Directory::parse(&bytes, &first).expect("directory");
    // Act
    let result = directory.json_cell(&schema.fields[0], &second);
    // Assert
    assert!(matches!(
        result,
        Err(super::controlled_directory::HandoffError::Directory(
            CassieError::ResourceLimit(_)
        ))
    ));
    assert_eq!(second.current_query_memory_bytes(), 0);
    drop(directory);
    assert_eq!(first.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_admitted_directories_after_actual_sort_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let bytes = row(&[(2, TYPE_JSON, 2, 2), (1, TYPE_JSON, 0, 2)], b"{}[]");
    let mut moves = 0;
    // Act
    let result = Directory::parse_observing(&bytes, &controls, |progress| {
        if matches!(progress, Progress::IdSortMove) {
            moves += 1;
            cancel.cancel();
        }
    });
    // Assert
    assert_eq!(moves, 1);
    assert!(result.is_err());
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_admitted_directories_after_actual_range_sort_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let bytes = row(&[(1, TYPE_JSON, 2, 2), (2, TYPE_JSON, 0, 2)], b"{}[]");
    let mut moves = 0;
    // Act
    let result = Directory::parse_observing(&bytes, &controls, |progress| {
        if matches!(progress, Progress::RangeSortMove) {
            moves += 1;
            cancel.cancel();
        }
    });
    // Assert
    assert_eq!(moves, 1);
    assert!(result.is_err());
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_directory_sort_overlap() {
    // Arrange
    let bytes = row(&[(2, TYPE_JSON, 2, 2), (1, TYPE_JSON, 0, 2)], b"{}[]");
    let baseline = controls();
    let directory = Directory::parse(&bytes, &baseline).expect("directory");
    let peak = baseline.peak_query_memory_bytes();
    drop(directory);
    // Act
    let outcomes = [(peak, true), (peak - 1, false)].map(|(budget, succeeds)| {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let result = Directory::parse(&bytes, &controls);
        let admitted = result.is_ok();
        drop(result);
        (succeeds, admitted, controls.current_query_memory_bytes())
    });

    // Assert
    for (succeeds, admitted, retained) in outcomes {
        assert_eq!(admitted, succeeds);
        assert_eq!(retained, 0);
    }
}
