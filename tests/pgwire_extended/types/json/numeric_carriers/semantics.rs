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
