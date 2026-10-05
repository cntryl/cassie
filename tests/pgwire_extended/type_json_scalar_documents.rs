//! Exact unsigned JSON document numbers retain their selected document meaning.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;
use cassie::types::Value;

const MAX_DOCUMENT: &[u8] = b"18446744073709551615";
const NEIGHBOR_DOCUMENT: &[u8] = b"18446744073709551614";
const WIDE_INTEGER_TEXT: &[u8] = b"9007199254740993";
const WIDE_INTEGER_BINARY: &[u8] = &[0, 32, 0, 0, 0, 0, 0, 1];

#[test]
fn should_reject_non_numeric_aggregate_carriers_with_exact_wire_errors() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_aggregate_errors (doc JSON, label TEXT, flag BOOLEAN)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_aggregate_errors (doc, label, flag) VALUES ($1, 'first', true), ($2, 'second', false), ($3, 'third', NULL)",
            vec![
                Value::Json(serde_json::json!(2)),
                Value::Json(serde_json::json!("not a number")),
                Value::Json(serde_json::json!(3)),
            ],
        ),
    ];
    let cases = [
        ("SUM(doc)", "function sum(json) does not exist"),
        ("AVG(doc)", "function avg(json) does not exist"),
        ("SUM(label)", "function sum(text) does not exist"),
        ("AVG(label)", "function avg(text) does not exist"),
        ("SUM(flag)", "function sum(boolean) does not exist"),
        ("AVG(flag)", "function avg(boolean) does not exist"),
        (
            "SUM(CAST(doc AS TEXT))",
            "function sum(text) does not exist",
        ),
        (
            "AVG(CAST(doc AS TEXT))",
            "function avg(text) does not exist",
        ),
        (
            "SUM(CAST(doc AS JSON))",
            "function sum(text) does not exist",
        ),
        (
            "AVG(CAST(doc AS JSON))",
            "function avg(text) does not exist",
        ),
    ];
    let mut cycles = Vec::new();
    for (expression, _) in cases {
        cycles.push(fixture::execute_cycle(
            &format!("SELECT {expression} FROM json_aggregate_errors"),
            None,
            0,
            false,
        ));
        cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
    }

    // Act
    let batches = fixture::run_cycles("type-json-aggregate-errors", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((expression, expected), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        let frames = &pair[0];
        assert_eq!(
            wire::error_code(frames).as_deref(),
            Some("42883"),
            "{expression}"
        );
        let errors = frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1, "{expression}");
        let message = wire::parse_error_fields(&errors[0].1)
            .into_iter()
            .find_map(|(key, value)| (key == 'M').then_some(value))
            .expect("aggregate error message");
        assert_eq!(message, expected, "{expression}");
        assert_eq!(
            frames
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
                .count(),
            0,
            "failed Execute must not publish a result for {expression}"
        );
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("1".to_string())]]);
    }
}

#[test]
fn should_preserve_ordinary_json_numeric_aggregates_in_both_wire_formats() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_numeric_aggregates (doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_numeric_aggregates (doc) VALUES ($1), ($2), ($3)",
            vec![
                Value::Json(serde_json::json!(2)),
                Value::Json(serde_json::json!(3)),
                Value::Null,
            ],
        ),
    ];
    let cases: [(&str, &[u8], &[u8]); 2] = [
        ("SUM(doc)", b"5", &[64, 20, 0, 0, 0, 0, 0, 0]),
        ("AVG(doc)", b"2.5", &[64, 4, 0, 0, 0, 0, 0, 0]),
    ];
    let cycles = cases
        .into_iter()
        .flat_map(|(expression, _, _)| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    &format!("SELECT {expression} FROM json_numeric_aggregates"),
                    None,
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-numeric-aggregates", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((_, text, binary), group) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        for (format, frames) in [0, 1].into_iter().zip(group) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (701, 8, -1), format, true);
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(if format == 0 {
                    text
                } else {
                    binary
                })]
            );
        }
    }
}

#[test]
fn should_preserve_unsigned_json_scalar_documents_across_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE unsigned_json_documents (k INT, doc JSON)",
            Vec::new(),
        ),
        (
            "CREATE TABLE unsigned_json_bigints (k INT, wide BIGINT)",
            Vec::new(),
        ),
    ];
    let mut first = Vec::new();
    for input_format in [0, 1] {
        for result_format in [0, 1] {
            let key = input_format * 2 + result_format + 1;
            first.push(fixture::execute_cycle(
                &format!(
                    "INSERT INTO unsigned_json_documents (k, doc) VALUES ({key}, $1) RETURNING doc"
                ),
                Some((114, input_format, Some(MAX_DOCUMENT))),
                result_format,
                true,
            ));
            first.push(fixture::execute_cycle(
                &format!(
                    "INSERT INTO unsigned_json_bigints (k, wide) VALUES ({key}, $1) RETURNING wide"
                ),
                Some((
                    20,
                    input_format,
                    Some(if input_format == 0 {
                        WIDE_INTEGER_TEXT
                    } else {
                        WIDE_INTEGER_BINARY
                    }),
                )),
                result_format,
                true,
            ));
        }
    }
    let reads = [0, 1]
        .into_iter()
        .flat_map(|format| {
            [
                fixture::execute_cycle(
                    "SELECT doc FROM unsigned_json_documents WHERE k = 1",
                    None,
                    format,
                    true,
                ),
                fixture::execute_cycle(
                    "SELECT wide FROM unsigned_json_bigints WHERE k = 1",
                    None,
                    format,
                    true,
                ),
            ]
        })
        .collect::<Vec<_>>();
    first.extend(reads.clone());

    // Act
    let stages = fixture::run_cycles_across_restart(
        "type-json-scalar-u64-restart",
        &setup,
        vec![first, reads],
    );

    // Assert
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].len(), 12);
    assert_eq!(stages[1].len(), 4);
    for (index, frames) in stages[0].iter().enumerate() {
        let format = if index < 8 {
            (index / 2) % 2
        } else {
            (index - 8) / 2
        };
        assert_number_roundtrip(
            frames,
            index % 2 == 0,
            i16::try_from(format).expect("format"),
        );
    }
    for (index, frames) in stages[1].iter().enumerate() {
        assert_number_roundtrip(
            frames,
            index % 2 == 0,
            i16::try_from(index / 2).expect("format"),
        );
    }
}

#[test]
fn should_preserve_unsigned_json_numeric_predicates() {
    // Arrange
    let setup = unsigned_json_predicate_setup();
    let inputs = [(MAX_DOCUMENT, vec![1, 2]), (NEIGHBOR_DOCUMENT, vec![3])];
    let cycles = inputs
        .iter()
        .flat_map(|(body, _)| {
            [0, 1].into_iter().flat_map(move |input_format| {
                [0, 1].map(move |format| {
                    fixture::execute_cycle(
                        "SELECT k FROM unsigned_json_predicates WHERE doc = $1 ORDER BY k",
                        Some((114, input_format, Some(body))),
                        format,
                        true,
                    )
                })
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-scalar-u64-predicate", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 8);
    for ((_, expected), group) in inputs.iter().zip(batches.as_chunks::<4>().0) {
        for (index, frames) in group.iter().enumerate() {
            let format = i16::try_from(index % 2).expect("format");
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (23, 4, -1), format, true);
            let rows = expected
                .iter()
                .map(|key: &i32| {
                    if format == 0 {
                        fixture::one_field_row(key.to_string().as_bytes())
                    } else {
                        fixture::one_field_row(&key.to_be_bytes())
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(fixture::data_row_payloads(frames), rows);
        }
    }
}

#[test]
fn should_group_unsigned_json_scalar_documents_without_rounding() {
    // Arrange
    let setup = unsigned_json_predicate_setup();
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle(
                "SELECT doc, COUNT(*) AS n FROM unsigned_json_predicates GROUP BY doc ORDER BY doc",
                None,
                format,
                true,
            )
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles("type-json-scalar-u64-groups", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 2);
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        let descriptions = frames
            .iter()
            .filter(|(tag, _)| *tag == b'T')
            .map(|(_, body)| wire::parse_row_description(body))
            .collect::<Vec<_>>();
        assert_eq!(descriptions.len(), 2);
        for (index, columns) in descriptions.into_iter().enumerate() {
            assert_eq!(columns.len(), 2);
            assert_eq!(
                (
                    columns[0].type_oid,
                    columns[0].type_size,
                    columns[0].type_mod
                ),
                (114, -1, -1)
            );
            assert_eq!(
                (
                    columns[1].type_oid,
                    columns[1].type_size,
                    columns[1].type_mod
                ),
                (20, 8, -1)
            );
            for column in columns {
                assert_eq!(column.format_code, if index == 0 { 0 } else { format });
            }
        }
        let expected = [(NEIGHBOR_DOCUMENT, 1_i64), (MAX_DOCUMENT, 2_i64)].map(|(doc, count)| {
            let mut row = fixture::one_field_row(doc);
            row[1] = 2;
            let count_field = if format == 0 {
                fixture::one_field_row(count.to_string().as_bytes())
            } else {
                fixture::one_field_row(&count.to_be_bytes())
            };
            row.extend_from_slice(&count_field[2..]);
            row
        });
        assert_eq!(fixture::data_row_payloads(&frames), expected.to_vec());
    }
}

#[test]
fn should_preserve_signed_json_numeric_key_normalization() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE signed_json_number_controls (doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO signed_json_number_controls (doc) VALUES ($1), ($2)",
            vec![
                Value::Json(serde_json::from_str("1").expect("integer JSON document")),
                Value::Json(serde_json::from_str("1.0").expect("float JSON document")),
            ],
        ),
    ];
    let queries = [
        "SELECT COUNT(*) AS n FROM signed_json_number_controls GROUP BY doc",
        "SELECT COUNT(*) AS n FROM signed_json_number_controls WHERE CAST(doc AS FLOAT) = 1",
    ];
    let cycles = queries
        .into_iter()
        .flat_map(|sql| [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true)))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-signed-key-sibling", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for (index, frames) in batches.iter().enumerate() {
        let format = i16::try_from(index % 2).expect("format");
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (20, 8, -1), format, true);
        let expected = if format == 0 {
            fixture::one_field_row(b"2")
        } else {
            fixture::one_field_row(&2_i64.to_be_bytes())
        };
        assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
    }
}

fn assert_number_roundtrip(frames: &fixture::Frames, json: bool, format: i16) {
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(
        frames,
        if json { (114, -1, -1) } else { (20, 8, -1) },
        format,
        true,
    );
    let bytes = if json {
        MAX_DOCUMENT
    } else if format == 0 {
        WIDE_INTEGER_TEXT
    } else {
        WIDE_INTEGER_BINARY
    };
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(bytes)]
    );
}

fn unsigned_json_predicate_setup() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        (
            "CREATE TABLE unsigned_json_predicates (k INT, doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO unsigned_json_predicates (k, doc) VALUES (1, $1), (2, $1), (3, $2)",
            vec![
                Value::Json(serde_json::from_slice(MAX_DOCUMENT).expect("exact JSON u64 maximum")),
                Value::Json(
                    serde_json::from_slice(NEIGHBOR_DOCUMENT).expect("exact adjacent JSON u64"),
                ),
            ],
        ),
    ]
}

#[test]
fn should_preserve_unsigned_json_documents_in_conflict_excluded_rows() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE unsigned_json_conflicts (k INT PRIMARY KEY, doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO unsigned_json_conflicts (k, doc) VALUES (1, '0')",
            Vec::new(),
        ),
    ];
    let cases = [MAX_DOCUMENT, NEIGHBOR_DOCUMENT];
    let cycles = cases.into_iter().flat_map(|body| {
        [0, 1].into_iter().flat_map(move |input_format| {
            [0, 1].map(move |format| {
                fixture::execute_cycle(
                    "INSERT INTO unsigned_json_conflicts (k, doc) VALUES (1, $1) ON CONFLICT (k) DO UPDATE SET doc = excluded.doc RETURNING doc",
                    Some((114, input_format, Some(body))),
                    format,
                    true,
                )
            })
        })
    }).collect();

    // Act
    let batches = fixture::run_cycles("type-json-u64-excluded-row", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 8);
    for (body, group) in cases.into_iter().zip(batches.as_chunks::<4>().0) {
        for (index, frames) in group.iter().enumerate() {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(
                frames,
                (114, -1, -1),
                i16::try_from(index % 2).expect("format"),
                true,
            );
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(body)]
            );
        }
    }
}

#[test]
fn should_preserve_ordinary_json_scalar_predicate_carriers() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE ordinary_json_number_controls (k INT, doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO ordinary_json_number_controls (k, doc) VALUES (1, $1), (2, $2), (3, $3)",
            vec![
                Value::Json(
                    serde_json::from_str("9007199254740993").expect("exact signed JSON number"),
                ),
                Value::Json(serde_json::from_str("3.5").expect("floating JSON document")),
                Value::Json(serde_json::from_str("false").expect("Boolean JSON document")),
            ],
        ),
    ];
    let cases = [
        (
            "SELECT doc FROM ordinary_json_number_controls WHERE CAST(doc AS BIGINT) = 9007199254740993",
            b"9007199254740993".as_slice(),
        ),
        (
            "SELECT doc FROM ordinary_json_number_controls WHERE CAST(doc AS FLOAT) = 3.5",
            b"3.5".as_slice(),
        ),
        (
            "SELECT doc FROM ordinary_json_number_controls WHERE CAST(doc AS BOOLEAN) = FALSE",
            b"false".as_slice(),
        ),
    ];
    let cycles = cases
        .into_iter()
        .flat_map(|(sql, _)| [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true)))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-ordinary-scalar-predicates", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 6);
    for ((_, body), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (114, -1, -1), format, true);
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(body)]
            );
        }
    }
}
