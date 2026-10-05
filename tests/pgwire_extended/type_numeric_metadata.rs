//! Numeric text adapters retain their input origin without adopting a result ABI.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_reject_nullable_numeric_adapters_at_implicit_boolean_sinks() {
    // Arrange
    let statements = [
        "SELECT 1 WHERE COALESCE($1, TRUE)",
        "WITH c AS (SELECT $1 AS v) SELECT 1 FROM c WHERE v",
        "SELECT 1 FROM (SELECT $1 AS v) AS c WHERE v",
    ];
    let cases = [700, 1700]
        .into_iter()
        .flat_map(|oid| statements.map(|sql| (oid, sql)))
        .collect::<Vec<_>>();
    let mut cycles = Vec::new();
    for (oid, sql) in &cases {
        cycles.push(fixture::execute_cycle(sql, Some((*oid, 0, None)), 0, false));
        cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-null-origin", &[], cycles);

    // Assert
    for ((oid, sql), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        let rejected = &pair[0];
        assert_eq!(
            wire::error_code(rejected).as_deref(),
            Some("42601"),
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

#[test]
fn should_preserve_explicit_boolean_casts_of_numeric_null_adapters() {
    // Arrange
    let cases = [700, 1700]
        .into_iter()
        .flat_map(|oid| [0, 1].map(|format| (oid, format)))
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(oid, format)| {
            fixture::execute_cycle(
                "SELECT CAST($1 AS BOOLEAN) AS flag",
                Some((*oid, 0, None)),
                *format,
                true,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-numeric-null-explicit-cast", &[], cycles);

    // Assert
    for ((oid, format), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (16, 1, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![vec![0, 1, 255, 255, 255, 255]]
        );
        assert_parameter_oid(&frames, oid);
    }
}

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
fn should_require_current_type_casts_for_decoder_only_result_parameters() {
    // Arrange
    let statements = [
        "SELECT $1 AS v",
        "SELECT $1 AS v LIMIT 0",
        "WITH c AS (SELECT $1 AS v) SELECT v FROM c",
        "SELECT v FROM (SELECT $1 AS v) AS c",
    ];
    let inputs = [None, Some(&b"2"[..]), Some(&b"2.5"[..])];
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
                    cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-output-cast-boundary", &[], cycles);

    // Assert
    for pair in batches.as_chunks::<2>().0 {
        fixture::assert_unsupported_before_metadata(&pair[0]);
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("1".to_string())]]);
    }
}

fn assert_parameter_oid(frames: &fixture::Frames, expected: i32) {
    let (_, payload) = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(payload), vec![expected]);
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
                    let expected =
                        input.or_else(|| sql.contains("COALESCE").then_some(&b"2.5"[..]));
                    cases.push((oid, format, expected));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-fixed-warm", &[], cycles);

    // Assert
    for ((oid, format, value), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (701, 8, -1), format, true);
        assert_parameter_oid(&frames, oid);
        let expected = value.map_or_else(
            || vec![0, 1, 255, 255, 255, 255],
            |text| {
                if format == 0 {
                    fixture::one_field_row(text)
                } else {
                    let value = if text == b"2" { 2.0_f64 } else { 2.5_f64 };
                    fixture::one_field_row(&value.to_be_bytes())
                }
            },
        );
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected]);
    }
}

#[test]
fn should_reject_numeric_origins_in_value_contributing_branches() {
    // Arrange
    let statements = [
        "SELECT 1 AS v UNION ALL SELECT $1 AS v",
        "WITH x(v) AS (SELECT $1 AS v) SELECT * FROM x",
        "WITH RECURSIVE x(v) AS (SELECT 1 UNION ALL SELECT $1 FROM x WHERE v < 1) SELECT v FROM x",
        "SELECT LAG($1) OVER () AS v",
    ];
    let mut cycles = Vec::new();
    for oid in [700, 1700] {
        for sql in statements {
            for input in [Some(&b"2"[..]), Some(&b"2.5"[..]), None] {
                cycles.push(fixture::execute_cycle(sql, Some((oid, 0, input)), 0, true));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-exported-origin", &[], cycles);

    // Assert
    for (index, frames) in batches.into_iter().enumerate() {
        assert_eq!(
            wire::error_code(&frames).as_deref(),
            Some("0A000"),
            "exported origin case {index}: {:?}",
            frames
                .iter()
                .filter(|(tag, _)| *tag == b'E')
                .map(|(_, body)| wire::parse_error_fields(body))
                .collect::<Vec<_>>()
        );
        fixture::assert_unsupported_before_metadata(&frames);
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
    for oid in [700, 1700] {
        for value in [Some(&b"2.5"[..]), Some(&b"2"[..]), None] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((oid, 0, value)),
                0,
                false,
            ));
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
    for frames in &batches[1..batches.len() - 1] {
        fixture::assert_unsupported_before_metadata(frames);
    }
}

#[test]
fn should_preserve_fixed_numeric_operation_outputs() {
    // Arrange
    let statements = [
        ("SELECT ABS($1) AS v", 700, false),
        ("SELECT SUM($1) AS v", 700, false),
        ("SELECT AVG($1) AS v", 1700, false),
        ("SELECT COUNT($1) AS v", 1700, true),
    ];
    let inputs: [(Option<&[u8]>, Option<f64>); 3] = [
        (Some(b"2"), Some(2.0)),
        (Some(b"2.5"), Some(2.5)),
        (None, None),
    ];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for (sql, oid, count) in statements {
        for format in [0, 1] {
            for (input, value) in inputs {
                cycles.push(fixture::execute_cycle(
                    sql,
                    Some((oid, 0, input)),
                    format,
                    true,
                ));
                cases.push((sql, oid, format, value, count));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-fixed-operation-boundary", &[], cycles);

    // Assert
    for ((sql, oid, format, value, count), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(
            &frames,
            (if count { 20 } else { 701 }, 8, -1),
            format,
            true,
        );
        assert_parameter_oid(&frames, oid);
        let expected = if count {
            let count = i64::from(value.is_some());
            if format == 1 {
                fixture::one_field_row(&count.to_be_bytes())
            } else {
                fixture::one_field_row(count.to_string().as_bytes())
            }
        } else {
            value.map_or_else(
                || vec![0, 1, 255, 255, 255, 255],
                |value| {
                    if format == 1 {
                        fixture::one_field_row(&value.to_be_bytes())
                    } else {
                        fixture::one_field_row(value.to_string().as_bytes())
                    }
                },
            )
        };
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected], "{sql}");
    }
}

#[test]
fn should_reject_adapter_forwarding_outputs() {
    // Arrange
    let statements = [
        ("SELECT ABS($1) AS v", 1700),
        ("SELECT SUM($1) AS v", 1700),
        ("SELECT MIN($1) AS v", 700),
        ("SELECT COALESCE($1, NULL) AS v", 700),
        ("SELECT CASE WHEN TRUE THEN $1 ELSE NULL END AS v", 700),
    ];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for (sql, oid) in statements {
        for format in [0, 1] {
            for input in [Some(&b"2"[..]), Some(&b"2.5"[..]), None] {
                for describe in [false, true] {
                    cycles.push(fixture::execute_cycle(
                        sql,
                        Some((oid, 0, input)),
                        format,
                        describe,
                    ));
                    cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
                    cases.push((sql, oid, format, describe));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-forwarding-boundary", &[], cycles);

    // Assert
    for ((sql, oid, format, describe), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(
            wire::error_code(&pair[0]).as_deref(),
            Some("0A000"),
            "{oid}:{format}:{describe}:{sql}"
        );
        fixture::assert_unsupported_before_metadata(&pair[0]);
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("1".to_string())]]);
    }
}

#[test]
fn should_propagate_numeric_origin_across_recursive_output_positions() {
    // Arrange
    let raw = "WITH RECURSIVE x(a,b,c) AS (SELECT 1,2,$1 UNION ALL SELECT b,c,a FROM x WHERE FALSE) SELECT a FROM x";
    let cast = "WITH RECURSIVE x(a,b,c) AS (SELECT 1,2,$1 UNION ALL SELECT b,c,a FROM x WHERE FALSE) SELECT CAST(a AS INT) AS v FROM x";
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for oid in [700, 1700] {
        for format in [0, 1] {
            for input in [Some(&b"2"[..]), Some(&b"2.5"[..]), None] {
                cycles.push(fixture::execute_cycle(
                    raw,
                    Some((oid, 0, input)),
                    format,
                    true,
                ));
                cycles.push(fixture::execute_cycle(
                    cast,
                    Some((oid, 0, input)),
                    format,
                    true,
                ));
                cases.push((oid, format));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-numeric-recursive-positional-origin", &[], cycles);

    // Assert
    for ((oid, format), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        fixture::assert_unsupported_before_metadata(&pair[0]);
        fixture::assert_success(&pair[1]);
        fixture::assert_single_column_descriptors(&pair[1], (23, 4, -1), format, true);
        assert_parameter_oid(&pair[1], oid);
        let expected = if format == 1 {
            fixture::one_field_row(&1_i32.to_be_bytes())
        } else {
            fixture::one_field_row(b"1")
        };
        assert_eq!(fixture::data_row_payloads(&pair[1]), vec![expected]);
    }
}

#[test]
fn should_preserve_failed_transaction_priority_over_numeric_output_rejection() {
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
        assert_eq!(wire::error_code(failed).as_deref(), Some("22000"), "{oid}");
        let (_, error) = failed.iter().find(|(tag, _)| *tag == b'E').expect("error");
        assert!(wire::parse_error_fields(error)
            .contains(&('M', "transaction is failed; rollback required".to_string())));
        assert_eq!(failed.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        assert!(!failed
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        assert_eq!(
            failed.last().map(|frame| frame.1.as_slice()),
            Some(&b"E"[..])
        );
        fixture::assert_success(&group[3]);
        fixture::assert_unsupported_before_metadata(&group[4]);
        fixture::assert_success(&group[5]);
        assert_eq!(
            wire::data_rows(&group[5]),
            vec![vec![Some("1".to_string())]]
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
