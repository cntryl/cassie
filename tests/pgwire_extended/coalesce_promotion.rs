use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_promote_coalesce_before_arithmetic_in_text_and_binary_wire_values() {
    // Arrange
    let cases = [
        (
            "SELECT COALESCE(n, f) AS v FROM records",
            "9007199254740992",
            9_007_199_254_740_992_f64,
        ),
        (
            "SELECT COALESCE(n, f) - 1 AS v FROM records",
            "9007199254740991",
            9_007_199_254_740_991_f64,
        ),
        (
            "WITH c AS (SELECT COALESCE(n, f) AS v FROM records) SELECT v - 1 AS v FROM c",
            "9007199254740991",
            9_007_199_254_740_991_f64,
        ),
    ];
    let cycles = [0, 1]
        .into_iter()
        .flat_map(|format| {
            cases
                .iter()
                .map(move |(sql, _, _)| fixture::execute_cycle(sql, None, format, true))
        })
        .collect();

    // Act
    let batches = fixture::run_cycles(
        "coalesce-promotion-wire",
        &[
            ("CREATE TABLE records (n BIGINT, f FLOAT)", vec![]),
            (
                "INSERT INTO records VALUES (9007199254740993, NULL)",
                vec![],
            ),
        ],
        cycles,
    );

    // Assert
    for (format, group) in [0, 1].into_iter().zip(batches.chunks(cases.len())) {
        for ((_, text, value), frames) in cases.iter().zip(group) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (701, 8, -1), format, true);
            if format == 0 {
                assert_eq!(
                    wire::data_rows(frames),
                    vec![vec![Some((*text).to_string())]]
                );
            } else {
                let mut expected = vec![0, 1];
                expected.extend_from_slice(&8_i32.to_be_bytes());
                expected.extend_from_slice(&value.to_be_bytes());
                assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
            }
        }
    }
}

#[test]
fn should_preserve_nonfinite_float_parameters_without_implicit_explicit_casts() {
    // Arrange
    let cases = [
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
        (f64::NAN, "NaN"),
        (-0.0_f64, "-0"),
    ];
    let sql = "SELECT COALESCE(CASE WHEN TRUE THEN $1 ELSE n END, n) AS v FROM records";
    let cycles = [0, 1]
        .into_iter()
        .flat_map(|format| {
            cases.iter().map(move |(value, _)| {
                fixture::execute_cycle(
                    sql,
                    Some((701, 1, Some(&value.to_be_bytes()))),
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles(
        "coalesce-promotion-nonfinite",
        &[
            ("CREATE TABLE records (n BIGINT)", vec![]),
            ("INSERT INTO records VALUES (NULL)", vec![]),
        ],
        cycles,
    );

    // Assert
    for (format, group) in [0, 1].into_iter().zip(batches.chunks(cases.len())) {
        for ((value, text), frames) in cases.iter().zip(group) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (701, 8, -1), format, true);
            if format == 0 {
                assert_eq!(wire::data_rows(frames), vec![vec![Some((*text).into())]]);
            } else {
                let mut expected = vec![0, 1];
                expected.extend_from_slice(&8_i32.to_be_bytes());
                expected.extend_from_slice(&value.to_be_bytes());
                assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
            }
        }
    }
}

#[test]
fn should_promote_coalesce_integer_parameters_after_scoped_type_resolution() {
    // Arrange
    let value = 9_007_199_254_740_993_i64.to_be_bytes();
    let cases = [
        "SELECT COALESCE($1, f) - 1 AS v FROM records",
        "WITH c AS (SELECT COALESCE($1, f) AS v FROM records) SELECT v - 1 AS v FROM c",
        "SELECT COALESCE($1, f) - 1 AS v FROM (SELECT f FROM records) AS d",
        "SELECT COALESCE(CASE WHEN TRUE THEN $1 ELSE CAST(NULL AS FLOAT) END, CAST(NULL AS FLOAT)) - 1 AS v",
        "WITH c AS (SELECT CASE WHEN TRUE THEN $1 ELSE CAST(NULL AS FLOAT) END AS v) SELECT COALESCE(v, CAST(NULL AS FLOAT)) - 1 AS v FROM c",
        "SELECT COALESCE(v, CAST(NULL AS FLOAT)) - 1 AS v FROM (SELECT CASE WHEN TRUE THEN $1 ELSE CAST(NULL AS FLOAT) END AS v) AS d",
    ];
    let cycles = [0, 1]
        .into_iter()
        .flat_map(|format| {
            let value = &value;
            cases.iter().map(move |sql| {
                fixture::execute_cycle(sql, Some((20, 1, Some(value))), format, true)
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles(
        "coalesce-promotion-parameter",
        &[
            ("CREATE TABLE records (f FLOAT)", vec![]),
            ("INSERT INTO records VALUES (NULL)", vec![]),
        ],
        cycles,
    );

    // Assert
    for (format, group) in [0, 1].into_iter().zip(batches.chunks(cases.len())) {
        for frames in group {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (701, 8, -1), format, true);
            if format == 0 {
                assert_eq!(
                    wire::data_rows(frames),
                    vec![vec![Some("9007199254740991".into())]]
                );
            } else {
                let mut expected = vec![0, 1];
                expected.extend_from_slice(&8_i32.to_be_bytes());
                expected.extend_from_slice(&9_007_199_254_740_991_f64.to_be_bytes());
                assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
            }
        }
    }
}

#[test]
fn should_reject_incompatible_later_coalesce_parameter_even_on_empty_input() {
    // Arrange
    let cases = [
        "SELECT COALESCE(n, f, $1) AS v FROM records",
        "WITH c AS (SELECT n, f FROM records) SELECT COALESCE(n, f, $1) AS v FROM c",
    ];
    let cycles = cases
        .iter()
        .map(|sql| fixture::execute_cycle(sql, Some((25, 0, Some(b"text"))), 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles(
        "coalesce-promotion-incompatible",
        &[("CREATE TABLE records (n BIGINT, f FLOAT)", vec![])],
        cycles,
    );

    // Assert
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("42601"));
        assert!(frames.iter().any(|(tag, body)| *tag == b'E'
            && String::from_utf8_lossy(body).contains("incompatible COALESCE result types")));
        assert!(wire::data_rows(&frames).is_empty());
    }
}

#[test]
fn should_promote_coalesce_after_inferred_integer_parameter_cast() {
    // Arrange
    let sql = "WITH c AS (SELECT COALESCE(CAST($1 AS BIGINT), f) AS v FROM records) SELECT v - 1 AS v FROM c";
    let cycles = [0, 1]
        .into_iter()
        .map(|format| {
            fixture::execute_cycle(sql, Some((0, 0, Some(b"9007199254740993"))), format, true)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles(
        "coalesce-promotion-inferred",
        &[
            ("CREATE TABLE records (f FLOAT)", vec![]),
            ("INSERT INTO records VALUES (NULL)", vec![]),
        ],
        cycles,
    );

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (701, 8, -1), format, true);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&parameters.1), vec![20]);
        if format == 0 {
            assert_eq!(
                wire::data_rows(frames),
                vec![vec![Some("9007199254740991".into())]]
            );
        } else {
            let mut expected = vec![0, 1];
            expected.extend_from_slice(&8_i32.to_be_bytes());
            expected.extend_from_slice(&9_007_199_254_740_991_f64.to_be_bytes());
            assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
        }
    }
}

#[test]
fn should_retain_float_domain_for_null_parameters_and_forwarded_case_carriers() {
    // Arrange
    let integer = 9_007_199_254_740_993_i64.to_be_bytes();
    let cases = [
        "SELECT COALESCE(CASE WHEN TRUE THEN $1 ELSE $2 END, $2) - 1 AS v",
        "WITH c AS (SELECT CASE WHEN TRUE THEN $1 ELSE $2 END AS v) SELECT COALESCE(v, $2) - 1 AS v FROM c",
        "SELECT COALESCE(v, $2) - 1 AS v FROM (SELECT CASE WHEN TRUE THEN $1 ELSE $2 END AS v) AS d",
    ];
    let cycles = [0, 1]
        .into_iter()
        .flat_map(|format| {
            let integer = &integer;
            cases.iter().map(move |sql| {
                vec![
                    wire::parse_frame_with_types("", sql, &[20, 701]),
                    wire::describe_statement_frame(""),
                    wire::bind_frame_with_formats("", "", &[1], &[Some(integer), None], &[format]),
                    wire::describe_portal_frame(""),
                    wire::execute_frame(""),
                    wire::sync_frame(),
                ]
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("coalesce-promotion-null-provenance", &[], cycles);

    // Assert
    for (format, group) in [0, 1].into_iter().zip(batches.chunks(cases.len())) {
        for frames in group {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (701, 8, -1), format, true);
            if format == 0 {
                assert_eq!(
                    wire::data_rows(frames),
                    vec![vec![Some("9007199254740991".into())]]
                );
            } else {
                let mut expected = vec![0, 1];
                expected.extend_from_slice(&8_i32.to_be_bytes());
                expected.extend_from_slice(&9_007_199_254_740_991_f64.to_be_bytes());
                assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
            }
        }
    }
}

#[test]
fn should_preserve_explicit_float_cast_errors_for_nonfinite_coalesce_values() {
    // Arrange
    let value = f64::INFINITY.to_be_bytes();
    let sql = "SELECT CAST(COALESCE(n, $1) AS FLOAT) AS v FROM records";
    let cycles = [0, 1]
        .into_iter()
        .map(|format| fixture::execute_cycle(sql, Some((701, 1, Some(&value))), format, true))
        .collect();

    // Act
    let batches = fixture::run_cycles(
        "coalesce-promotion-explicit-float-cast",
        &[
            ("CREATE TABLE records (n BIGINT)", vec![]),
            ("INSERT INTO records VALUES (NULL)", vec![]),
        ],
        cycles,
    );

    // Assert
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("22000"));
        assert!(frames.iter().any(|(tag, body)| *tag == b'E'
            && String::from_utf8_lossy(body).contains("cannot cast value to FLOAT")));
        assert!(wire::data_rows(&frames).is_empty());
    }
}

#[test]
fn should_keep_prepared_integer_arithmetic_exact_before_coalesce() {
    // Arrange
    let integer = 9_007_199_254_740_993_i64.to_be_bytes();
    let zero = 0_i64.to_be_bytes();
    let cases = [
        "SELECT COALESCE($1 + $2, 0) AS v",
        "WITH c AS (SELECT $1 + $2 AS v) SELECT COALESCE(v, 0) AS v FROM c",
        "SELECT COALESCE(v, 0) AS v FROM (SELECT $1 + $2 AS v) AS d",
    ];
    let cycles = [0, 1]
        .into_iter()
        .flat_map(|format| {
            let integer = &integer;
            let zero = &zero;
            cases.iter().map(move |sql| {
                vec![
                    wire::parse_frame_with_types("", sql, &[20, 20]),
                    wire::describe_statement_frame(""),
                    wire::bind_frame_with_formats(
                        "",
                        "",
                        &[1],
                        &[Some(integer), Some(zero)],
                        &[format],
                    ),
                    wire::describe_portal_frame(""),
                    wire::execute_frame(""),
                    wire::sync_frame(),
                ]
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("coalesce-promotion-integer-parameters", &[], cycles);

    // Assert
    for (format, group) in [0, 1].into_iter().zip(batches.chunks(cases.len())) {
        for frames in group {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (20, 8, -1), format, true);
            if format == 0 {
                assert_eq!(
                    wire::data_rows(frames),
                    vec![vec![Some("9007199254740993".into())]]
                );
            } else {
                let mut expected = vec![0, 1];
                expected.extend_from_slice(&8_i32.to_be_bytes());
                expected.extend_from_slice(&integer);
                assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
            }
        }
    }
}
