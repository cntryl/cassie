#[test]
fn should_preserve_constrained_numeric_assignment_metadata() {
    // Arrange
    const WIDE: i64 = 9_007_199_254_740_993;
    let setup = [("CREATE TABLE numeric_target (k BIGINT)", Vec::new())];
    let wide_text = WIDE.to_string();
    let cases = [0, 1]
        .into_iter()
        .flat_map(|format| [false, true].map(|is_null| (format, is_null)))
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(format, is_null)| {
            fixture::execute_cycle(
                "INSERT INTO numeric_target (k) VALUES ($1) RETURNING k",
                Some((1700, 0, (!is_null).then_some(wide_text.as_bytes()))),
                *format,
                true,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-numeric-constrained-assignment", &setup, cycles);

    // Assert
    for ((format, is_null), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (20, 8, -1), format, true);
        assert_parameter_oid(&frames, 1700);
        let expected = if is_null {
            vec![0, 1, 255, 255, 255, 255]
        } else if format == 1 {
            fixture::one_field_row(&WIDE.to_be_bytes())
        } else {
            fixture::one_field_row(wide_text.as_bytes())
        };
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected]);
    }
}

#[test]
fn should_preserve_postgresql_numeric_operation_result_contracts() {
    // Arrange
    let statements = [
        ("SELECT ABS($1) AS v", 700, 700, 4, false),
        ("SELECT SUM($1) AS v", 700, 700, 4, false),
        ("SELECT AVG($1) AS v", 1700, 1700, -1, false),
        ("SELECT COUNT($1) AS v", 1700, 20, 8, true),
    ];
    let inputs: [Option<&[u8]>; 3] = [Some(b"2"), Some(b"2.5"), None];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for (sql, parameter_oid, result_oid, result_size, count) in statements {
        for format in [0, 1] {
            for input in inputs {
                cycles.push(fixture::execute_cycle(
                    sql,
                    Some((parameter_oid, 0, input)),
                    format,
                    true,
                ));
                cases.push((
                    sql,
                    parameter_oid,
                    result_oid,
                    result_size,
                    format,
                    input,
                    count,
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-fixed-operation-boundary", &[], cycles);

    // Assert
    for ((sql, parameter_oid, result_oid, result_size, format, input, count), frames) in
        cases.into_iter().zip(batches)
    {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(
            &frames,
            (result_oid, result_size, -1),
            format,
            true,
        );
        assert_parameter_oid(&frames, parameter_oid);
        let expected = if count {
            let count = i64::from(input.is_some());
            if format == 1 {
                fixture::one_field_row(&count.to_be_bytes())
            } else {
                fixture::one_field_row(count.to_string().as_bytes())
            }
        } else if result_oid == 1700 {
            input.map_or_else(
                || vec![0, 1, 255, 255, 255, 255],
                |input| {
                    if format == 1 {
                        let numeric = if input == b"2" {
                            [0, 1, 0, 0, 0, 0, 0, 16, 0, 2].as_slice()
                        } else {
                            [0, 2, 0, 0, 0, 0, 0, 16, 0, 2, 19, 136].as_slice()
                        };
                        fixture::one_field_row(numeric)
                    } else {
                        let text = if input == b"2" {
                            b"2.0000000000000000".as_slice()
                        } else {
                            b"2.5000000000000000".as_slice()
                        };
                        fixture::one_field_row(text)
                    }
                },
            )
        } else {
            input.map_or_else(
                || vec![0, 1, 255, 255, 255, 255],
                |input| {
                    if format == 1 {
                        let text = std::str::from_utf8(input).expect("numeric input is UTF-8");
                        if result_oid == 701 {
                            let value = text.parse::<f64>().expect("valid float8 result");
                            fixture::one_field_row(&value.to_be_bytes())
                        } else {
                            let value = text.parse::<f32>().expect("valid float4 result");
                            fixture::one_field_row(&value.to_be_bytes())
                        }
                    } else {
                        fixture::one_field_row(input)
                    }
                },
            )
        };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![expected],
            "{parameter_oid}:{sql}"
        );
    }
}

#[test]
fn should_return_numeric_result_for_abs_numeric_parameter() {
    // Arrange
    let sql = "SELECT ABS($1) AS v";
    let cycles = vec![fixture::execute_cycle(
        sql,
        Some((1700, 0, Some(b"2.5"))),
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("type-numeric-abs-sum-results", &[], cycles);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1700, -1, -1), 0, true);
    assert_parameter_oid(frames, 1700);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"2.5")]
    );
}

#[test]
fn should_preserve_explicit_float_boundaries_beside_generated_coercions() {
    // Arrange
    let statements = [
        "SELECT COALESCE($1, CAST(NULL AS FLOAT)) AS v",
        "SELECT COALESCE($1, CAST(CASE WHEN FALSE THEN NULL ELSE NULL END AS FLOAT)) AS v",
        "SELECT GREATEST($1, CAST(NULL AS FLOAT)) AS v",
        "SELECT CAST(NULLIF($1, NULL) AS FLOAT) AS v",
    ];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for oid in [700, 1700] {
        for sql in statements {
            for format in [0, 1] {
                for input in [Some(&b"2.5"[..]), None] {
                    cases.push((oid, sql, format, input.is_some()));
                    cycles.push(fixture::execute_cycle(
                        sql,
                        Some((oid, 0, input)),
                        format,
                        true,
                    ));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-explicit-float-coercions", &[], cycles);

    // Assert
    for ((oid, sql, format, present), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (701, 8, -1), format, true);
        assert_parameter_oid(&frames, oid);
        let expected = if present {
            if format == 0 {
                fixture::one_field_row(b"2.5")
            } else {
                fixture::one_field_row(&2.5_f64.to_be_bytes())
            }
        } else {
            vec![0, 1, 255, 255, 255, 255]
        };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![expected],
            "{oid}:{sql}"
        );
    }
}

#[test]
fn should_keep_count_star_fixed_with_numeric_predicate_parameters() {
    // Arrange
    let setup = [
        ("CREATE TABLE numeric_count_rows (k INT)", Vec::new()),
        (
            "INSERT INTO numeric_count_rows (k) VALUES (1), (2)",
            Vec::new(),
        ),
    ];
    let sql = "SELECT COUNT(*) AS v FROM numeric_count_rows WHERE k >= $1";
    let inputs: [(Option<&[u8]>, i64); 3] = [(Some(b"2"), 1), (Some(b"2.5"), 0), (None, 0)];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for oid in [700, 1700] {
        for format in [0, 1] {
            for (input, count) in inputs {
                cycles.push(fixture::execute_cycle(
                    sql,
                    Some((oid, 0, input)),
                    format,
                    true,
                ));
                cases.push((oid, format, count));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-count-star-policy", &setup, cycles);

    // Assert
    for ((oid, format, count), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (20, 8, -1), format, true);
        assert_parameter_oid(&frames, oid);
        let expected = if format == 1 {
            fixture::one_field_row(&count.to_be_bytes())
        } else {
            fixture::one_field_row(count.to_string().as_bytes())
        };
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected]);
    }
}

#[test]
fn should_preserve_numeric_parameter_identity_through_pg_typeof_wire_formats() {
    // Arrange
    let numeric_binary = [
        0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x13, 0x88,
    ];
    let cases = [(0, 0), (0, 1), (1, 0), (1, 1)];
    let cycles = cases
        .iter()
        .map(|(input_format, result_format)| {
            fixture::execute_cycle(
                "SELECT pg_typeof($1) AS actual_type",
                Some((
                    1700,
                    *input_format,
                    Some(if *input_format == 0 {
                        &b"2.5"[..]
                    } else {
                        &numeric_binary[..]
                    }),
                )),
                *result_format,
                true,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-numeric-pg-typeof-wire", &[], cycles);

    // Assert
    for ((input_format, result_format), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (2_206, 4, -1), result_format, true);
        assert_parameter_oid(&frames, 1700);
        let expected_value = if result_format == 0 {
            fixture::one_field_row(b"numeric")
        } else {
            fixture::one_field_row(&1700_i32.to_be_bytes())
        };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![expected_value],
            "numeric input format {input_format}, result format {result_format}"
        );
    }
}

/// `real` and `numeric` parameters keep their declared result types.
fn numeric_descriptor(oid: i32) -> (i32, i16, i32) {
    if oid == 700 {
        (700, 4, -1)
    } else {
        (1700, -1, -1)
    }
}

fn null_row() -> Vec<u8> {
    vec![0, 1, 255, 255, 255, 255]
}

/// The PostgreSQL send format of the `2` and `2.5` fixtures.
fn numeric_binary_row(oid: i32, text: &[u8]) -> Vec<u8> {
    if oid == 700 {
        let value = if text == b"2" { 2.0_f32 } else { 2.5_f32 };
        fixture::one_field_row(&value.to_be_bytes())
    } else if text == b"2" {
        fixture::one_field_row(&[0, 1, 0, 0, 0, 0, 0, 0, 0, 2])
    } else {
        fixture::one_field_row(&[0, 2, 0, 0, 0, 0, 0, 1, 0, 2, 0x13, 0x88])
    }
}

// PostgreSQL 18.6 rejects a numeric WHERE operand with 42804. Cassie reports
// the shared 42601 boolean-context error for direct and CTE operands of every
// non-Boolean type (oracle-corrections.md).
#[test]
fn should_reject_numeric_parameters_at_implicit_boolean_sinks() {
    // Arrange
    let statements = [
        ("SELECT 1 WHERE COALESCE($1, TRUE)", "42601"),
        ("WITH c AS (SELECT $1 AS v) SELECT 1 FROM c WHERE v", "42601"),
        ("SELECT 1 FROM (SELECT $1 AS v) AS c WHERE v", "42804"),
    ];
    let cases = [700, 1700]
        .into_iter()
        .flat_map(|oid| statements.map(|(sql, code)| (oid, sql, code)))
        .collect::<Vec<_>>();
    let mut cycles = Vec::new();
    for (oid, sql, _) in &cases {
        cycles.push(fixture::execute_cycle(sql, Some((*oid, 0, None)), 0, false));
        cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-null-origin", &[], cycles);

    // Assert
    for ((oid, sql, code), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        let rejected = &pair[0];
        assert_eq!(
            wire::error_code(rejected).as_deref(),
            Some(code),
            "{oid}: {sql}"
        );
        assert!(!rejected
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        assert_eq!(
            rejected.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("1".to_string())]]);
    }
}

// PostgreSQL 18.6 defines no `real` or `numeric` to `boolean` cast (42846).
#[test]
fn should_reject_explicit_boolean_casts_of_numeric_parameters() {
    // Arrange
    let cases = [700, 1700]
        .into_iter()
        .flat_map(|oid| [0, 1].map(|format| (oid, format)))
        .collect::<Vec<_>>();
    let mut cycles = Vec::new();
    for (oid, format) in &cases {
        cycles.push(fixture::execute_cycle(
            "SELECT CAST($1 AS BOOLEAN) AS flag",
            Some((*oid, 0, None)),
            *format,
            true,
        ));
        cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-null-explicit-cast", &[], cycles);

    // Assert
    for ((oid, format), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(
            wire::error_code(&pair[0]).as_deref(),
            Some("42846"),
            "{oid}:{format}"
        );
        assert!(!pair[0]
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        fixture::assert_success(&pair[1]);
    }
}

#[test]
fn should_return_numeric_parameters_with_their_declared_result_types() {
    // Arrange
    let statements = [
        "SELECT $1 AS v",
        "SELECT $1 AS v LIMIT 0",
        "WITH c AS (SELECT $1 AS v) SELECT v FROM c",
        "SELECT v FROM (SELECT $1 AS v) AS c",
    ];
    let inputs = [None, Some(&b"2"[..]), Some(&b"2.5"[..])];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for oid in [700, 1700] {
        for sql in statements {
            for input in inputs {
                for describe in [false, true] {
                    cycles.push(fixture::execute_cycle(
                        sql,
                        Some((oid, 0, input)),
                        0,
                        describe,
                    ));
                    cases.push((oid, sql, input, describe));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-output-cast-boundary", &[], cycles);

    // Assert
    for ((oid, sql, input, describe), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, numeric_descriptor(oid), 0, describe);
        let expected = if sql.contains("LIMIT 0") {
            Vec::new()
        } else {
            vec![input.map_or_else(null_row, fixture::one_field_row)]
        };
        assert_eq!(fixture::data_row_payloads(&frames), expected, "{oid}:{sql}");
    }
}

#[test]
fn should_preserve_fixed_numeric_output_across_warm_bindings() {
    // Arrange
    let statements = [
        "SELECT CAST($1 AS FLOAT) AS v",
        "WITH x AS (SELECT $1 AS v) SELECT CAST(v AS FLOAT) FROM x",
        "SELECT CAST(v AS FLOAT) FROM (SELECT $1 AS v) AS x",
        "SELECT COALESCE($1, 2.5) AS v",
    ];
    let inputs = [Some(&b"2"[..]), Some(&b"2.5"[..]), None, Some(&b"2"[..])];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for oid in [1700, 700, 1700] {
        for sql in statements {
            for format in [0, 1] {
                for input in inputs {
                    cycles.push(fixture::execute_cycle(
                        sql,
                        Some((oid, 0, input)),
                        format,
                        true,
                    ));
                    let coalesce = sql.contains("COALESCE");
                    let expected = input.or_else(|| coalesce.then_some(&b"2.5"[..]));
                    cases.push((oid, coalesce, format, expected));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-fixed-warm", &[], cycles);

    // Assert
    for ((oid, coalesce, format, value), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        // COALESCE(numeric, 2.5) is numeric and COALESCE(real, 2.5) is real.
        let descriptor = if coalesce {
            numeric_descriptor(oid)
        } else {
            (701, 8, -1)
        };
        fixture::assert_single_column_descriptors(&frames, descriptor, format, true);
        assert_parameter_oid(&frames, oid);
        let expected = value.map_or_else(null_row, |text| {
            if format == 0 {
                fixture::one_field_row(text)
            } else if coalesce {
                numeric_binary_row(oid, text)
            } else {
                let value = if text == b"2" { 2.0_f64 } else { 2.5_f64 };
                fixture::one_field_row(&value.to_be_bytes())
            }
        });
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected]);
    }
}

#[test]
fn should_type_numeric_parameters_in_value_contributing_branches() {
    // Arrange
    let statements = [
        "SELECT 1 AS v UNION ALL SELECT $1 AS v",
        "WITH x(v) AS (SELECT $1 AS v) SELECT * FROM x",
        "SELECT LAG($1) OVER () AS v",
    ];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for oid in [700, 1700] {
        for sql in statements {
            for input in [Some(&b"2"[..]), Some(&b"2.5"[..]), None] {
                cycles.push(fixture::execute_cycle(sql, Some((oid, 0, input)), 0, true));
                cases.push((oid, sql, input));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-exported-origin", &[], cycles);

    // Assert
    for ((oid, sql, input), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, numeric_descriptor(oid), 0, true);
        let parameter = input.map_or_else(null_row, fixture::one_field_row);
        let mut expected = if sql.contains("UNION") {
            vec![fixture::one_field_row(b"1"), parameter]
        } else if sql.contains("LAG") {
            vec![null_row()]
        } else {
            vec![parameter]
        };
        // UNION ALL row order is unspecified.
        let mut actual = fixture::data_row_payloads(&frames);
        expected.sort();
        actual.sort();
        assert_eq!(actual, expected, "{oid}:{sql}");
    }
}

#[test]
fn should_reject_recursive_terms_that_widen_integer_columns_to_numeric() {
    // Arrange
    let sql = "WITH RECURSIVE x(v) AS (SELECT 1 UNION ALL SELECT $1 FROM x WHERE v < 1) SELECT v FROM x";
    let mut cycles = Vec::new();
    for oid in [700, 1700] {
        cycles.push(fixture::execute_cycle(sql, Some((oid, 0, Some(b"2"))), 0, true));
        cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-recursive-widening", &[], cycles);

    // Assert
    for ((_, name), pair) in [(700, "real"), (1700, "numeric")]
        .into_iter()
        .zip(batches.as_chunks::<2>().0)
    {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("42804"));
        let (_, error) = pair[0].iter().find(|(tag, _)| *tag == b'E').expect("error");
        assert!(wire::parse_error_fields(error).contains(&(
            'M',
            format!(
                "recursive query \"x\" column 1 has type integer in non-recursive term but type {name} overall"
            )
        )));
        assert!(!pair[0]
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        fixture::assert_success(&pair[1]);
    }
}

#[test]
fn should_revalidate_numeric_output_after_supported_float_cache_warmup() {
    // Arrange
    let mut cycles = vec![fixture::execute_cycle(
        "SELECT $1 AS v",
        Some((701, 0, Some(b"2.5"))),
        1,
        true,
    )];
    let mut cases = Vec::new();
    for oid in [700, 1700] {
        for value in [Some(&b"2.5"[..]), Some(&b"2"[..]), None] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((oid, 0, value)),
                0,
                false,
            ));
            cases.push((oid, value));
        }
    }
    cycles.push(fixture::execute_cycle(
        "SELECT $1 AS v",
        Some((701, 0, Some(b"2.5"))),
        1,
        true,
    ));

    // Act
    let batches = fixture::run_cycles("type-numeric-cache-origin", &[], cycles);

    // Assert
    for frames in [&batches[0], batches.last().expect("warm recovery")] {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (701, 8, -1), 1, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&2.5_f64.to_be_bytes())]
        );
    }
    for ((oid, value), frames) in cases.into_iter().zip(&batches[1..batches.len() - 1]) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, numeric_descriptor(oid), 0, false);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![value.map_or_else(null_row, fixture::one_field_row)]
        );
    }
}

#[test]
fn should_forward_numeric_parameters_through_normalized_conditionals() {
    // Arrange
    let statements = [
        "SELECT NULLIF(COALESCE($1, NULL), NULL) AS v",
        "SELECT GREATEST(COALESCE($1, NULL), NULL) AS v",
        "SELECT LEAST(COALESCE($1, NULL), NULL) AS v",
        "WITH c AS (SELECT COALESCE($1, NULL) AS v) SELECT GREATEST(v, NULL) FROM c",
        "SELECT NULLIF(v, NULL) FROM (SELECT COALESCE($1, NULL) AS v) AS c",
    ];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for oid in [700, 1700] {
        for sql in statements {
            for input in [Some(&b"2.5"[..]), None] {
                for describe in [false, true] {
                    cases.push((oid, sql, input, describe));
                    cycles.push(fixture::execute_cycle(
                        sql,
                        Some((oid, 0, input)),
                        0,
                        describe,
                    ));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-normalized-forwarding", &[], cycles);

    // Assert
    for ((oid, sql, input, describe), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, numeric_descriptor(oid), 0, describe);
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![input.map_or_else(null_row, fixture::one_field_row)],
            "{oid}:{describe}:{sql}"
        );
    }
}

#[test]
fn should_forward_real_parameters_through_forwarding_outputs() {
    // Arrange
    let statements = [
        "SELECT MIN($1) AS v",
        "SELECT COALESCE($1, NULL) AS v",
        "SELECT CASE WHEN TRUE THEN $1 ELSE NULL END AS v",
    ];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for sql in statements {
        for format in [0, 1] {
            for input in [Some(&b"2"[..]), Some(&b"2.5"[..]), None] {
                for describe in [false, true] {
                    cycles.push(fixture::execute_cycle(
                        sql,
                        Some((700, 0, input)),
                        format,
                        describe,
                    ));
                    cases.push((sql, format, input, describe));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-forwarding-boundary", &[], cycles);

    // Assert
    for ((sql, format, input, describe), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (700, 4, -1), format, describe);
        let expected = input.map_or_else(null_row, |text| {
            if format == 0 {
                fixture::one_field_row(text)
            } else {
                numeric_binary_row(700, text)
            }
        });
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![expected],
            "{format}:{describe}:{sql}"
        );
    }
}

#[test]
fn should_reject_recursive_positions_that_rotate_numeric_into_integer_columns() {
    // Arrange
    let raw = "WITH RECURSIVE x(a,b,c) AS (SELECT 1,2,$1 UNION ALL SELECT b,c,a FROM x WHERE FALSE) SELECT a FROM x";
    let cast = "WITH RECURSIVE x(a,b,c) AS (SELECT 1,2,$1 UNION ALL SELECT b,c,a FROM x WHERE FALSE) SELECT CAST(a AS INT) AS v FROM x";
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for (oid, name) in [(700, "real"), (1700, "numeric")] {
        for sql in [raw, cast] {
            cycles.push(fixture::execute_cycle(sql, Some((oid, 0, Some(b"2"))), 0, true));
            cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
            cases.push(name);
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-recursive-positional-origin", &[], cycles);

    // Assert
    for (name, pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("42804"));
        let (_, error) = pair[0].iter().find(|(tag, _)| *tag == b'E').expect("error");
        assert!(wire::parse_error_fields(error).contains(&(
            'M',
            format!(
                "recursive query \"x\" column 2 has type integer in non-recursive term but type {name} overall"
            )
        )));
        fixture::assert_success(&pair[1]);
    }
}

#[test]
fn should_preserve_failed_transaction_priority_over_numeric_parameter_results() {
    // Arrange
    let mut cycles = Vec::new();
    for oid in [700, 1700] {
        cycles.push(vec![wire::simple_query_frame("BEGIN")]);
        cycles.push(vec![wire::simple_query_frame("SELECT 1 / 0")]);
        cycles.push(fixture::execute_cycle(
            "SELECT $1 AS v",
            Some((oid, 0, Some(b"2.5"))),
            0,
            false,
        ));
        cycles.push(vec![wire::simple_query_frame("ROLLBACK")]);
        cycles.push(fixture::execute_cycle(
            "SELECT $1 AS v",
            Some((oid, 0, Some(b"2.5"))),
            0,
            false,
        ));
        cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-failed-transaction-priority", &[], cycles);

    // Assert
    for (oid, group) in [700, 1700].into_iter().zip(batches.as_chunks::<6>().0) {
        assert_eq!(wire::error_code(&group[0]), None, "BEGIN under {oid}");
        assert_eq!(
            group[0].last().map(|frame| frame.1.as_slice()),
            Some(&b"T"[..])
        );
        assert_eq!(wire::error_code(&group[1]).as_deref(), Some("22012"));
        assert_eq!(
            group[1].last().map(|frame| frame.1.as_slice()),
            Some(&b"E"[..])
        );
        let failed = &group[2];
        assert_eq!(wire::error_code(failed).as_deref(), Some("25P02"), "{oid}");
        let (_, error) = failed.iter().find(|(tag, _)| *tag == b'E').expect("error");
        assert!(wire::parse_error_fields(error).contains(&(
            'M',
            "current transaction is aborted, commands ignored until end of transaction block"
                .to_string()
        )));
        assert_eq!(failed.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        assert!(!failed
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        assert_eq!(
            failed.last().map(|frame| frame.1.as_slice()),
            Some(&b"E"[..])
        );
        fixture::assert_success(&group[3]);
        fixture::assert_success(&group[4]);
        assert_eq!(
            fixture::data_row_payloads(&group[4]),
            vec![fixture::one_field_row(b"2.5")]
        );
        fixture::assert_success(&group[5]);
        assert_eq!(
            wire::data_rows(&group[5]),
            vec![vec![Some("1".to_string())]]
        );
    }
}
