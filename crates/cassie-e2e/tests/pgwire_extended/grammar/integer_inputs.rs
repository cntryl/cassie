//! Checked integer text input and arithmetic error recovery on actual pgwire.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_integer_wire_values_given_radix_text_when_binary_portals_execute() {
    // Arrange
    let cases = [
        (21, "0x_7fff", 32_767_i64, 2),
        (21, "-0o_100000", -32_768, 2),
        (23, " +2_147_483_647 ", 2_147_483_647, 4),
        (23, "-0x8000_0000", -2_147_483_648, 4),
        (23, "\u{b}42\u{b}", 42, 4),
        (23, "\u{c}42\u{c}", 42, 4),
        (21, "+0X_7FFF", 32767, 2),
        (21, "-0B_1000000000000000", -32768, 2),
        (21, "00_1", 1, 2),
        (20, "9_007_199_254_740_993", 9_007_199_254_740_993, 8),
        (20, "-0x8000_0000_0000_0000", i64::MIN, 8),
    ];
    // Act
    let outcomes = cases.map(|(oid, input, expected, width)| {
        (
            run_script(
                "integer-text-binary",
                vec![
                    Step::ParseFlush(wire::parse_frame_with_types("n", "SELECT $1 AS n", &[oid])),
                    Step::ThroughReady(vec![
                        wire::describe_statement_frame("n"),
                        wire::sync_frame(),
                    ]),
                    Step::ThroughReady(vec![
                        wire::bind_frame_with_formats(
                            "out",
                            "n",
                            &[],
                            &[Some(input.as_bytes())],
                            &[1],
                        ),
                        wire::describe_portal_frame("out"),
                        wire::execute_frame("out"),
                        wire::sync_frame(),
                    ]),
                ],
            ),
            oid,
            expected,
            width,
        )
    });
    // Assert
    for (batches, oid, expected, width) in outcomes {
        for batch in &batches {
            assert_eq!(wire::error_code(batch), None);
        }
        let parameters = batches[1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("ParameterDescription");
        assert_eq!(wire::parse_parameter_description(&parameters.1), vec![oid]);
        let description = batches[2]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!(
            (column.type_oid, column.type_size, column.type_mod),
            (oid, width, -1)
        );
        let row = batches[2]
            .iter()
            .find(|frame| frame.0 == b'D')
            .expect("binary integer row");
        let bytes = expected.to_be_bytes();
        let start = bytes.len() - usize::try_from(width).expect("fixed integer width");
        assert_eq!(&row.1[6..], &bytes[start..]);
    }
}

#[test]
fn should_prioritize_integer_input_errors_given_malformed_text_when_the_statement_is_rebound() {
    // Arrange
    let cases = [
        (21, "0x__1", "22P02"),
        (23, "\u{1c}42\u{1c}", "22P02"),
        (21, "0x1__0", "22P02"),
        (21, "0b2", "22P02"),
        (21, "32768x", "22P02"),
        (21, "0x8000x", "22P02"),
        (21, "0x80000x", "22003"),
        (21, "99999x", "22003"),
        (23, "2147483648x", "22P02"),
        (23, "9999999999999999999999x", "22003"),
        (23, "\u{a0}42\u{a0}", "22P02"),
        (20, "9223372036854775808", "22003"),
        (20, "9223372036854775808x", "22P02"),
        (20, "999999999999999999999x", "22003"),
        (20, "1_", "22P02"),
    ];
    // Act
    let outcomes = cases.map(|(oid, input, state)| {
        (
            run_script(
                "integer-text-error-rebind",
                vec![
                    Step::ParseFlush(wire::parse_frame_with_types("n", "SELECT $1 AS n", &[oid])),
                    Step::ThroughReady(vec![
                        wire::bind_frame_with_formats(
                            "bad",
                            "n",
                            &[],
                            &[Some(input.as_bytes())],
                            &[],
                        ),
                        wire::execute_frame("bad"),
                        wire::sync_frame(),
                    ]),
                    Step::ThroughReady(vec![
                        wire::bind_frame_with_formats("good", "n", &[], &[Some(b"12")], &[]),
                        wire::execute_frame("good"),
                        wire::sync_frame(),
                    ]),
                ],
            ),
            state,
        )
    });
    // Assert
    for (batches, state) in outcomes {
        assert_eq!(wire::error_code(&batches[0]), None);
        assert_eq!(wire::error_code(&batches[1]).as_deref(), Some(state));
        assert_eq!(
            wire::data_rows(&batches[1]),
            Vec::<Vec<Option<String>>>::new()
        );
        assert_eq!(wire::error_code(&batches[2]), None);
        assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("12".into())]]);
    }
}

#[test]
fn should_preserve_integer_error_states_given_constant_casts_when_empty_outputs_are_planned() {
    // Arrange
    let cases = [
        ("SELECT CAST(32768 AS SMALLINT) WHERE FALSE", "22003"),
        ("SELECT CAST(2147483648 AS INT) WHERE FALSE", "22003"),
        ("SELECT CAST('9223372036854775808' AS BIGINT)", "22003"),
        ("SELECT CAST('0x__1' AS INT) WHERE FALSE", "22P02"),
        ("VALUES (32767::SMALLINT + 1::SMALLINT)", "22003"),
        ("VALUES (2147483647 + 1), (1::BIGINT)", "22003"),
        ("VALUES ((-2147483648)::INT4 / (-1)::INT4) LIMIT 0", "22003"),
        ("VALUES (1 / 0) LIMIT 0", "22012"),
    ];
    // Act
    let outcomes = cases.map(|(sql, state)| {
        (
            run_script(
                "integer-cast-errors",
                vec![
                    Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
                    Step::ThroughReady(vec![wire::simple_query_frame(
                        "SELECT CAST('0x_FF' AS INT)",
                    )]),
                ],
            ),
            state,
        )
    });
    // Assert
    for (batches, state) in outcomes {
        assert_eq!(wire::error_code(&batches[0]).as_deref(), Some(state));
        assert_eq!(
            wire::data_rows(&batches[0]),
            Vec::<Vec<Option<String>>>::new()
        );
        assert_eq!(wire::error_code(&batches[1]), None);
        assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("255".into())]]);
    }
}

#[test]
fn should_diagnose_resolved_integer_inputs_given_false_filters_when_boolean_aggregates_are_bound() {
    // Arrange
    let sql = "SELECT BOOL_AND(n) FROM (VALUES (1 + 2)) AS v(n) WHERE FALSE";
    // Act
    let batches = run_script(
        "integer-values-aggregate-input",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    let error = batches[0]
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("type ErrorResponse");
    assert!(wire::parse_error_fields(&error.1)
        .iter()
        .any(|(kind, message)| *kind == 'M'
            && message == "function bool_and(integer) does not exist"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
}

#[test]
fn should_preserve_existing_integer_storage_given_values_arithmetic_when_views_are_rebound_after_restart(
) {
    // Arrange
    let setup = [
        ("CREATE VIEW integer_source_view AS SELECT n FROM (VALUES (1::SMALLINT + 2::SMALLINT)) AS v(n)", vec![]),
        ("CREATE TABLE integer_source_rows (n SMALLINT)", vec![]),
        ("INSERT INTO integer_source_rows SELECT n FROM integer_source_view", vec![]),
    ];
    let stages = (0..2)
        .map(|_| {
            vec![
                vec![wire::simple_query_frame(
                    "SELECT n FROM integer_source_view",
                )],
                vec![wire::simple_query_frame(
                    "SELECT n FROM integer_source_rows",
                )],
            ]
        })
        .collect();
    // Act
    let outcomes = crate::support_type_metadata_contract::run_cycles_across_restart(
        "integer-values-view-restart",
        &setup,
        stages,
    );
    // Assert
    assert_eq!(outcomes.len(), 2);
    for stage in outcomes {
        for frames in stage {
            assert_eq!(wire::error_code(&frames), None);
            let description = frames
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("integer RowDescription");
            let column = &wire::parse_row_description(&description.1)[0];
            assert_eq!(
                (column.type_oid, column.type_size, column.type_mod),
                (21, 2, -1)
            );
            assert_eq!(wire::data_rows(&frames), vec![vec![Some("3".into())]]);
        }
    }
}

#[test]
fn should_prioritize_utf8_validation_given_invalid_text_bytes_when_integer_inputs_are_bound() {
    // Arrange
    let cases: [(i32, &[u8], &str); 8] = [
        (23, b"42\0suffix", "0x00"),
        (21, b"999999\0suffix", "0x00"),
        (25, b"a\0b", "0x00"),
        (23, b"42\xff", "0xff"),
        (23, b"42\0\xff", "0x00"),
        (21, b"\xc0a", "0xc0 0x61"),
        (21, b"\xe0ab", "0xe0 0x61 0x62"),
        (21, b"\xf0abc", "0xf0 0x61 0x62 0x63"),
    ];
    // Act
    let outcomes = cases.map(|(oid, input, suffix)| {
        (
            run_script(
                "integer-input-encoding",
                vec![
                    Step::ParseFlush(wire::parse_frame_with_types("n", "SELECT $1 AS n", &[oid])),
                    Step::ThroughReady(vec![
                        wire::bind_frame_with_formats("bad", "n", &[], &[Some(input)], &[]),
                        wire::execute_frame("bad"),
                        wire::sync_frame(),
                    ]),
                    Step::ThroughReady(vec![
                        wire::bind_frame_with_formats("good", "n", &[], &[Some(b"12")], &[]),
                        wire::execute_frame("good"),
                        wire::sync_frame(),
                    ]),
                ],
            ),
            suffix,
        )
    });
    // Assert
    for (batches, suffix) in outcomes {
        assert_eq!(wire::error_code(&batches[0]), None);
        assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22021"));
        let error = batches[1]
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("encoding ErrorResponse");
        let expected = format!("invalid byte sequence for encoding \"UTF8\": {suffix}");
        assert!(wire::parse_error_fields(&error.1)
            .iter()
            .any(|(kind, message)| *kind == 'M' && message == &expected));
        assert_eq!(
            wire::data_rows(&batches[1]),
            Vec::<Vec<Option<String>>>::new()
        );
        assert_eq!(wire::error_code(&batches[2]), None);
        assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("12".into())]]);
    }
}

#[test]
fn should_preserve_operator_domains_given_wider_values_columns_when_prepared_statements_are_rebound(
) {
    // Arrange
    let cases = [
        (21, 21, "$1 + 1::SMALLINT", "32767"),
        (23, 23, "$1 + 1", "2147483647"),
        (20, 20, "$1 + 1", "9223372036854775807"),
        (0, 23, "$1::INT4 + 1", "2147483647"),
    ];
    // Act
    let outcomes = cases.map(|(supplied, resolved, expression, input)| {
        let sql = format!("VALUES ({expression}), (1::BIGINT)");
        (
            run_script(
                "integer-operator-domain-rebind",
                vec![
                    Step::ParseFlush(wire::parse_frame_with_types("n", &sql, &[supplied])),
                    Step::ThroughReady(vec![
                        wire::describe_statement_frame("n"),
                        wire::sync_frame(),
                    ]),
                    Step::ThroughReady(vec![
                        wire::bind_frame_with_formats(
                            "bad",
                            "n",
                            &[],
                            &[Some(input.as_bytes())],
                            &[],
                        ),
                        wire::execute_frame("bad"),
                        wire::sync_frame(),
                    ]),
                    Step::ThroughReady(vec![
                        wire::bind_frame_with_formats("good", "n", &[], &[Some(b"7")], &[]),
                        wire::describe_portal_frame("good"),
                        wire::execute_frame("good"),
                        wire::sync_frame(),
                    ]),
                ],
            ),
            resolved,
        )
    });
    // Assert
    for (batches, resolved) in outcomes {
        assert_eq!(wire::error_code(&batches[0]), None);
        let parameters = batches[1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            vec![resolved]
        );
        assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22003"));
        assert_eq!(
            wire::data_rows(&batches[2]),
            Vec::<Vec<Option<String>>>::new()
        );
        assert_eq!(wire::error_code(&batches[3]), None);
        let description = batches[3]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("wider RowDescription");
        assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 20);
        assert_eq!(
            wire::data_rows(&batches[3]),
            vec![vec![Some("8".into())], vec![Some("1".into())]]
        );
    }
}

#[test]
fn should_apply_postfix_casts_before_integer_negation_given_signed_minima_when_queries_execute() {
    // Arrange
    let cases = [
        "SELECT -32768::SMALLINT",
        "SELECT -2147483648::INT4",
        "SELECT -9223372036854775808::BIGINT",
        "VALUES (-32768::SMALLINT), (1::BIGINT)",
        "SELECT 1 LIMIT -32768::SMALLINT",
    ];
    // Act
    let outcomes = cases.map(|sql| {
        run_script(
            "integer-postfix-sign-precedence",
            vec![
                Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
                Step::ThroughReady(vec![wire::simple_query_frame(
                    "SELECT CAST(-32768 AS SMALLINT), -1::SMALLINT",
                )]),
            ],
        )
    });
    // Assert
    for batches in outcomes {
        assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
        assert_eq!(
            wire::data_rows(&batches[0]),
            Vec::<Vec<Option<String>>>::new()
        );
        assert_eq!(wire::error_code(&batches[1]), None);
        assert_eq!(
            wire::data_rows(&batches[1]),
            vec![vec![Some("-32768".into()), Some("-1".into())]]
        );
    }
}
