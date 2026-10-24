#[test]
fn should_preserve_integer_array_text_endpoints_in_both_spellings() {
    // Arrange
    let cases: [IntegerArrayEndpoint; 3] = [
        (
            1005,
            b"{-32768,32767,NULL}",
            b"[-32768,32767,null]",
            b"[-32768,32767,null]",
            b"\0\0\0\x01\0\0\0\x01\0\0\0\x15\0\0\0\x03\0\0\0\x01\0\0\0\x02\x80\0\0\0\0\x02\x7f\xff\xff\xff\xff\xff",
        ),
        (
            1007,
            b"{-2147483648,2147483647,NULL}",
            b"[-2147483648,2147483647,null]",
            b"[-2147483648,2147483647,null]",
            b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x03\0\0\0\x01\0\0\0\x04\x80\0\0\0\0\0\0\x04\x7f\xff\xff\xff\xff\xff\xff\xff",
        ),
        (
            1016,
            b"{-9223372036854775808,9223372036854775807,9007199254740993,NULL}",
            b"[-9223372036854775808,9223372036854775807,9007199254740993,null]",
            b"[-9223372036854775808,9223372036854775807,9007199254740993,null]",
            b"\0\0\0\x01\0\0\0\x01\0\0\0\x14\0\0\0\x04\0\0\0\x01\0\0\0\x08\x80\0\0\0\0\0\0\0\0\0\0\x08\x7f\xff\xff\xff\xff\xff\xff\xff\0\0\0\x08\0\x20\0\0\0\0\0\x01\xff\xff\xff\xff",
        ),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (oid, brace, bracket, text, binary) in cases {
        for input in [brace, bracket] {
            for format in [0, 1] {
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((oid, 0, Some(input))),
                    format,
                    true,
                ));
                expectations.push((oid, format, if format == 0 { text } else { binary }));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-text-integer-endpoints", &[], cycles);

    // Assert
    for ((oid, format, expected), frames) in expectations.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (oid, -1, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(expected)]
        );
    }
}

#[test]
fn should_round_trip_escaped_array_text() {
    // Arrange
    let input = br#"{"comma,entry","quote\"inside","back\\slash"," padded "}"#;
    let cycle = fixture::execute_cycle(
        "SELECT $1::TEXT[] AS value",
        Some((1_009, 0, Some(input))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-text-quoting", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1_009, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(
            r#"["comma,entry","quote\"inside","back\\slash"," padded "]"#.to_owned()
        )]]
    );
}

#[test]
fn should_canonicalize_string_backed_array_text_elements_before_direct_output() {
    // Arrange
    let cases: [(i32, &[u8], &[u8]); 11] = [
        (1182, b"{2026-2-3,NULL}", b"[\"2026-02-03\",null]"),
        (1182, b"[\"2026-2-3\",null]", b"[\"2026-02-03\",null]"),
        (1183, b"{1:2:3.1,NULL}", b"[\"01:02:03.100000\",null]"),
        (1183, b"[\"1:2:3.1\",null]", b"[\"01:02:03.100000\",null]"),
        (
            1115,
            b"{\"2026-10-05T01:00:00+01:00\",NULL}",
            b"[\"2026-10-05T00:00:00.000000Z\",null]",
        ),
        (
            1115,
            b"[\"2026-10-05T01:00:00+01:00\",null]",
            b"[\"2026-10-05T00:00:00.000000Z\",null]",
        ),
        (
            2951,
            b"{550E8400-E29B-41D4-A716-446655440000,NULL}",
            b"[\"550e8400-e29b-41d4-a716-446655440000\",null]",
        ),
        (
            2951,
            b"[\"550E8400-E29B-41D4-A716-446655440000\",null]",
            b"[\"550e8400-e29b-41d4-a716-446655440000\",null]",
        ),
        (1001, b"{\"\\\\xABCD\",NULL}", b"[\"\\\\xabcd\",null]"),
        (1001, b"[\"\\\\xABCD\",null]", b"[\"\\\\xabcd\",null]"),
        (1000, b"{yes,NULL}", b"[true,null]"),
    ];
    let cycles = cases
        .iter()
        .map(|(oid, payload, _)| {
            fixture::execute_cycle("SELECT $1 AS v", Some((*oid, 0, Some(*payload))), 0, true)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-array-text-direct-canonical", &[], cycles);

    // Assert
    for ((oid, _, expected), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (oid, -1, -1), 0, true);
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(expected)]
        );
    }
}

#[test]
fn should_preserve_declared_float_array_text_rounding() {
    // Arrange
    let inputs = [
        b"{9007199254740993,NULL}".as_slice(),
        b"[9007199254740993,null]".as_slice(),
    ];
    let text = b"[9007199254740992.0,null]";
    let binary = b"\0\0\0\x01\0\0\0\x01\0\0\x02\xbd\0\0\0\x02\0\0\0\x01\0\0\0\x08\x43\x40\0\0\0\0\0\0\xff\xff\xff\xff";
    let cycles = inputs
        .into_iter()
        .flat_map(|input| {
            [0, 1].map(move |format| {
                fixture::execute_cycle("SELECT $1 AS v", Some((1022, 0, Some(input))), format, true)
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-array-float-input-rounding", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for (index, frames) in batches.iter().enumerate() {
        let format = i16::try_from(index % 2).expect("format");
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (1022, -1, -1), format, true);
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

#[test]
fn should_preserve_scalar_float_bits_in_both_array_spellings() {
    // Arrange
    let cases = [
        (
            b"51.248178375505404".as_slice(),
            b"\x40\x49\x9f\xc4\x4f\x1b\x2f\x60".as_slice(),
        ),
        (
            b"-93.31137037688033".as_slice(),
            b"\xc0\x57\x53\xed\x7e\x04\x69\x3b".as_slice(),
        ),
        (
            b"-36.573994842753436".as_slice(),
            b"\xc0\x42\x49\x78\xa9\xba\xd9\x6e".as_slice(),
        ),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (token, bits) in cases {
        let text = std::str::from_utf8(token).expect("decimal token");
        let inputs = [format!("{{{text},NULL}}"), format!("[{text},null]")];
        let mut binary = b"\0\0\0\x01\0\0\0\x01\0\0\x02\xbd\0\0\0\x02\0\0\0\x01\0\0\0\x08".to_vec();
        binary.extend_from_slice(bits);
        binary.extend_from_slice(b"\xff\xff\xff\xff");
        for format in [0, 1] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((701, 0, Some(token))),
                format,
                true,
            ));
            expectations.push((
                701,
                8,
                format,
                if format == 0 {
                    token.to_vec()
                } else {
                    bits.to_vec()
                },
            ));
            for input in &inputs {
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((1022, 0, Some(input.as_bytes()))),
                    format,
                    true,
                ));
                expectations.push((
                    1022,
                    -1,
                    format,
                    if format == 0 {
                        format!("[{text},null]").into_bytes()
                    } else {
                        binary.clone()
                    },
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-float-decimal-bits", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 18);
    for ((oid, length, format, payload), frames) in expectations.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, length, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&payload)]
        );
    }
}

#[test]
fn should_preserve_finite_float_array_tokens_before_output() {
    // Arrange
    let cases = [
        (
            "1.7976931348623157e308",
            b"\x7f\xef\xff\xff\xff\xff\xff\xff".as_slice(),
        ),
        (
            "-1.7976931348623157e308",
            b"\xff\xef\xff\xff\xff\xff\xff\xff".as_slice(),
        ),
        ("2.2250738585072014e-308", b"\0\x10\0\0\0\0\0\0".as_slice()),
        ("5e-324", b"\0\0\0\0\0\0\0\x01".as_slice()),
        ("-0.0", b"\x80\0\0\0\0\0\0\0".as_slice()),
        ("1e-400", b"\0\0\0\0\0\0\0\0".as_slice()),
        (
            "1.7976931348623158e308",
            b"\x7f\xef\xff\xff\xff\xff\xff\xff".as_slice(),
        ),
        ("2.4703282292062328e-324", b"\0\0\0\0\0\0\0\x01".as_slice()),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (token, bits) in cases {
        cycles.push(fixture::execute_cycle(
            "SELECT $1 AS v",
            Some((701, 0, Some(token.as_bytes()))),
            1,
            true,
        ));
        expectations.push((701, 8, bits.to_vec()));
        for input in [format!("{{{token},NULL}}"), format!("[{token},null]")] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((1022, 0, Some(input.as_bytes()))),
                1,
                true,
            ));
            let mut binary =
                b"\0\0\0\x01\0\0\0\x01\0\0\x02\xbd\0\0\0\x02\0\0\0\x01\0\0\0\x08".to_vec();
            binary.extend_from_slice(bits);
            binary.extend_from_slice(b"\xff\xff\xff\xff");
            expectations.push((1022, -1, binary));
        }
    }
    let invalid = [
        "[1e400]",
        r#"["51.248178375505404"]"#,
        "[true]",
        "[{}]",
        "[[]]",
        "[51.248178375505404,]",
        "[51.248178375505404] trailing",
        "[51.248178375505404,null",
    ];
    for input in invalid {
        for format in [0, 1] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((1022, 0, Some(input.as_bytes()))),
                format,
                false,
            ));
            cycles.push(fixture::execute_cycle("SELECT 1 AS v", None, 0, true));
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-float-finite-tokens", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 56);
    for ((oid, length, payload), frames) in expectations.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, length, -1), 1, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&payload)]
        );
    }
    for pair in batches[24..].as_chunks::<2>().0 {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("08P01"));
        assert!(!pair[0]
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        fixture::assert_success(&pair[1]);
        assert_eq!(
            fixture::data_row_payloads(&pair[1]),
            vec![fixture::one_field_row(b"1")]
        );
    }
}

#[test]
fn should_preserve_integer_array_negative_zero_in_both_spellings() {
    // Arrange
    let cases = [
        (1005, 21, b"\0\0".as_slice()),
        (1007, 23, b"\0\0\0\0".as_slice()),
        (1016, 20, b"\0\0\0\0\0\0\0\0".as_slice()),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (oid, element_oid, zero) in cases {
        let mut binary = b"\0\0\0\x01\0\0\0\x01".to_vec();
        binary.extend_from_slice(&i32::to_be_bytes(element_oid));
        binary.extend_from_slice(b"\0\0\0\x02\0\0\0\x01");
        binary.extend_from_slice(
            &i32::try_from(zero.len())
                .expect("integer width")
                .to_be_bytes(),
        );
        binary.extend_from_slice(zero);
        binary.extend_from_slice(b"\xff\xff\xff\xff");
        for input in [b"{-0,NULL}".as_slice(), b"[-0,null]".as_slice()] {
            for format in [0, 1] {
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((oid, 0, Some(input))),
                    format,
                    true,
                ));
                expectations.push((
                    oid,
                    format,
                    if format == 0 {
                        b"[0,null]".to_vec()
                    } else {
                        binary.clone()
                    },
                ));
            }
        }
    }
    for (oid, _, _) in cases {
        for input in [
            b"[-0.0]".as_slice(),
            b"[0.0]".as_slice(),
            b"[0e0]".as_slice(),
        ] {
            for format in [0, 1] {
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((oid, 0, Some(input))),
                    format,
                    false,
                ));
                cycles.push(fixture::execute_cycle("SELECT 1 AS v", None, 0, true));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-integer-negative-zero", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 48);
    for ((oid, format, payload), frames) in expectations.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, -1, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&payload)]
        );
    }
    for pair in batches[12..].as_chunks::<2>().0 {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("08P01"));
        assert!(!pair[0]
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        fixture::assert_success(&pair[1]);
        assert_eq!(
            fixture::data_row_payloads(&pair[1]),
            vec![fixture::one_field_row(b"1")]
        );
    }
}

#[test]
fn should_preserve_character_array_input_without_wire_modifiers() {
    // Arrange
    let inputs = [r#"{"éA  ",NULL}"#.as_bytes(), r#"["éA  ",null]"#.as_bytes()];
    let expected_text = r#"["éA  ",null]"#.as_bytes();
    let cases = [
        (1014, b"\0\0\0\x01\0\0\0\x01\0\0\x04\x12\0\0\0\x02\0\0\0\x01\0\0\0\x05\xc3\xa9A  \xff\xff\xff\xff".as_slice()),
        (1015, b"\0\0\0\x01\0\0\0\x01\0\0\x04\x13\0\0\0\x02\0\0\0\x01\0\0\0\x05\xc3\xa9A  \xff\xff\xff\xff".as_slice()),
    ];
    let cycles = cases
        .into_iter()
        .flat_map(|(oid, _)| {
            inputs.into_iter().flat_map(move |input| {
                [0, 1].map(move |format| {
                    fixture::execute_cycle(
                        "SELECT $1 AS v",
                        Some((oid, 0, Some(input))),
                        format,
                        true,
                    )
                })
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-array-character-unknown-modifier", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 8);
    for ((oid, binary), group) in cases.into_iter().zip(batches.as_chunks::<4>().0) {
        for (index, frames) in group.iter().enumerate() {
            let format = i16::try_from(index % 2).expect("format");
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (oid, -1, -1), format, true);
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(if format == 0 {
                    expected_text
                } else {
                    binary
                })]
            );
        }
    }
}

#[test]
fn should_filter_table_rows_with_any_array_predicates() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_any_rows (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_any_rows VALUES (1, $1), (2, $2), (3, $3)",
            vec![
                Value::Json(serde_json::json!([1, 2])),
                Value::Json(serde_json::json!([4, 5])),
                Value::Json(serde_json::json!([2, 3])),
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT id FROM issue_807_array_any_rows WHERE 2 = ANY(items) ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-any-table-predicate", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("1".to_owned())], vec![Some("3".to_owned())]]
    );
}

#[test]
fn should_accept_sql_standard_array_type_declarations() {
    // Arrange
    let create = fixture::execute_cycle(
        "CREATE TABLE issue_807_array_standard_declarations (fixed INTEGER ARRAY[4], flexible INTEGER ARRAY)",
        None,
        0,
        false,
    );
    let select = fixture::execute_cycle(
        "SELECT fixed, flexible FROM issue_807_array_standard_declarations",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-array-standard-declarations",
        &[],
        vec![create, select],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    let frames = &batches[1];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("declared array RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1007, 1007]
    );
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
}

#[test]
fn should_order_grouped_distinct_array_values() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_order_rows (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_order_rows VALUES (1, $1), (2, $2), (3, $3), (4, $4), (5, $5), (6, $6), (7, $7), (8, NULL)",
            vec![
                Value::Json(serde_json::json!([1, 2])),
                Value::Json(serde_json::json!([1, 2])),
                Value::Json(serde_json::json!([1, 2, 0])),
                Value::Json(serde_json::json!([1, 3])),
                Value::Json(serde_json::json!([2, 0])),
                Value::Json(serde_json::json!([[1, 2], [3, 4]])),
                Value::Json(serde_json::json!([0, 9])),
            ],
        ),
    ];
    let grouped = fixture::execute_cycle(
        "SELECT items, count(*) FROM issue_807_array_order_rows GROUP BY items ORDER BY items NULLS LAST",
        None,
        0,
        true,
    );
    let distinct = fixture::execute_cycle(
        "SELECT DISTINCT items FROM issue_807_array_order_rows ORDER BY items NULLS LAST",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-array-order-distinct-group",
        &setup,
        vec![grouped, distinct],
    );

    // Assert
    let grouped = &batches[0];
    fixture::assert_success(grouped);
    let description = grouped
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array grouping RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(1007, -1), (20, 8)]
    );
    assert_eq!(
        wire::data_rows(grouped),
        vec![
            vec![Some("{0,9}".to_owned()), Some("1".to_owned())],
            vec![Some("{1,2}".to_owned()), Some("2".to_owned())],
            vec![Some("{1,2,0}".to_owned()), Some("1".to_owned())],
            vec![Some("{{1,2},{3,4}}".to_owned()), Some("1".to_owned())],
            vec![Some("{1,3}".to_owned()), Some("1".to_owned())],
            vec![Some("{2,0}".to_owned()), Some("1".to_owned())],
            vec![None, Some("1".to_owned())],
        ]
    );

    let distinct = &batches[1];
    fixture::assert_success(distinct);
    fixture::assert_single_column_descriptors(distinct, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(distinct),
        vec![
            vec![Some("{0,9}".to_owned())],
            vec![Some("{1,2}".to_owned())],
            vec![Some("{1,2,0}".to_owned())],
            vec![Some("{{1,2},{3,4}}".to_owned())],
            vec![Some("{1,3}".to_owned())],
            vec![Some("{2,0}".to_owned())],
            vec![None],
        ]
    );
}

#[test]
fn should_preserve_array_extrema_wire_identity() {
    // Arrange
    let cycle = vec![
        wire::parse_frame_with_types("array_extrema", "SELECT min($1), max($2)", &[1007, 1007]),
        wire::describe_statement_frame("array_extrema"),
        wire::bind_frame("array_extrema_portal", "array_extrema", &["{1,2}", "{2,0}"]),
        wire::describe_portal_frame("array_extrema_portal"),
        wire::execute_frame("array_extrema_portal"),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("type-array-min-max-aggregates", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1007, 1007]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{1,2}".to_owned()), Some("{2,0}".to_owned())]]
    );
}
