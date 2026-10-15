//! Focused PostgreSQL 18 pgwire inventory gaps for #762, #796, #798, and #799.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_compare_two_column_subquery_rows_with_null_truth_values() {
    // Arrange
    let sql = "WITH candidates(a, b) AS (VALUES \
        (1::INT4, NULL::INT4), (1::INT4, 2::INT4), (2::INT4, 2::INT4)) \
        SELECT \
            ROW(1::INT4, 2::INT4) = \
                (SELECT a, b FROM candidates WHERE a = 1 AND b = 2) AS exact_equal, \
            ROW(1::INT4, 2::INT4) IN \
                (SELECT a, b FROM candidates WHERE a = 1) AS in_contains_equal, \
            ROW(1::INT4, NULL::INT4) = \
                (SELECT a, b FROM candidates WHERE a = 1 AND b IS NULL) AS null_field_equal, \
            ROW(1::INT4, NULL::INT4) IN \
                (SELECT a, b FROM candidates WHERE a = 1 AND b = 2) AS null_field_indeterminate, \
            ROW(1::INT4, NULL::INT4) IN \
                (SELECT a, b FROM candidates WHERE a = 2) AS nonnull_field_mismatch";
    let batches = run_script(
        "issue-762-row-subquery-null-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-subquery Boolean RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 5]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        [
            "exact_equal",
            "in_contains_equal",
            "null_field_equal",
            "null_field_indeterminate",
            "nonnull_field_mismatch",
        ]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            None,
            None,
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_comparison_matrix_to_subquery_quantifiers() {
    // Arrange
    let sql = "WITH candidates(value) AS (VALUES (1::INT4), (2::INT4), (3::INT4)) \
        SELECT \
            2 = ANY (SELECT value FROM candidates) AS any_eq_true, \
            2 = ANY (SELECT value FROM candidates WHERE value = 3) AS any_eq_false, \
            2 <> ANY (SELECT value FROM candidates) AS any_ne_true, \
            2 <> ANY (SELECT value FROM candidates WHERE value = 2) AS any_ne_false, \
            2 < ANY (SELECT value FROM candidates) AS any_lt_true, \
            2 < ANY (SELECT value FROM candidates WHERE value <= 2) AS any_lt_false, \
            2 <= ANY (SELECT value FROM candidates) AS any_lte_true, \
            2 <= ANY (SELECT value FROM candidates WHERE value = 1) AS any_lte_false, \
            2 > ANY (SELECT value FROM candidates) AS any_gt_true, \
            2 > ANY (SELECT value FROM candidates WHERE value = 3) AS any_gt_false, \
            2 >= ANY (SELECT value FROM candidates) AS any_gte_true, \
            2 >= ANY (SELECT value FROM candidates WHERE value = 3) AS any_gte_false, \
            2 = ALL (SELECT value FROM candidates WHERE value = 2) AS all_eq_true, \
            2 = ALL (SELECT value FROM candidates) AS all_eq_false, \
            2 <> ALL (SELECT value FROM candidates WHERE value = 1) AS all_ne_true, \
            2 <> ALL (SELECT value FROM candidates WHERE value = 2) AS all_ne_false, \
            2 < ALL (SELECT value FROM candidates WHERE value = 3) AS all_lt_true, \
            2 < ALL (SELECT value FROM candidates WHERE value = 1) AS all_lt_false, \
            2 <= ALL (SELECT value FROM candidates WHERE value >= 2) AS all_lte_true, \
            2 <= ALL (SELECT value FROM candidates WHERE value = 1) AS all_lte_false, \
            2 > ALL (SELECT value FROM candidates WHERE value = 1) AS all_gt_true, \
            2 > ALL (SELECT value FROM candidates WHERE value = 3) AS all_gt_false, \
            2 >= ALL (SELECT value FROM candidates WHERE value <= 2) AS all_gte_true, \
            2 >= ALL (SELECT value FROM candidates WHERE value = 3) AS all_gte_false";
    let batches = run_script(
        "issue-762-subquery-any-all-operator-matrix",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("ANY/ALL Boolean RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns.len(), 24);
    assert!(columns.iter().all(|column| column.type_oid == 16));
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_describe_chained_qualified_array_cast_expression() {
    // Arrange
    let sql = "SELECT $1::pg_catalog.int4::pg_catalog.text AS chained, \
        $2::pg_catalog.int4[] AS array_value";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "qualified_casts",
            sql,
            &[0, 0],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("qualified_casts"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "qualified_casts_portal",
                "qualified_casts",
                &["0042", "{1,2}"],
            ),
            wire::describe_portal_frame("qualified_casts_portal"),
            wire::execute_frame("qualified_casts_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-796-qualified-chained-casts-wire", steps);
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("cast ParameterDescription")
                .1
        ),
        vec![23, 1007]
    );
    let statement_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("cast statement RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    assert_eq!(
        statement_columns
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        [("chained", 25), ("array_value", 1007)]
    );
    assert_eq!(wire::error_code(executed), None);
    let portal_description = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("cast portal RowDescription");
    assert_eq!(
        wire::parse_row_description(&portal_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 1007]
    );
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("42".to_owned()), Some("{1,2}".to_owned())]]
    );
}

#[test]
fn should_distinguish_icu_collation_order_from_c_order_over_pgwire() {
    // Arrange
    let batches = run_script(
        "issue-796-non-c-collation-order",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 'ä' COLLATE \"C\" < 'z' COLLATE \"C\" AS c_order, \
                'ä' COLLATE \"und-x-icu\" < 'z' COLLATE \"und-x-icu\" AS icu_order",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("collation Boolean RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("f".to_owned()), Some("t".to_owned())]]
    );
}

#[test]
fn should_bind_pattern_operators_with_escape_over_pgwire() {
    // Arrange
    let sql = "SELECT $1 SIMILAR TO $2 ESCAPE $3 AS similar_match, \
        $4 NOT LIKE $5 ESCAPE $6 AS escaped_not_like";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "pattern_parameters",
            sql,
            &[25, 25, 25, 25, 25, 25],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("pattern_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "pattern_portal",
                "pattern_parameters",
                &["foo_7", "foo!_%", "!", "ab", "a!_%", "!"],
            ),
            wire::describe_portal_frame("pattern_portal"),
            wire::execute_frame("pattern_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-799-bound-similar-like-escape-wire", steps);
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("pattern ParameterDescription")
                .1
        ),
        vec![25; 6]
    );
    let statement_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("pattern statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(wire::error_code(executed), None);
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("t".to_owned()), Some("t".to_owned())]]
    );
}

#[test]
fn should_preserve_power_precedence_with_float8_metadata() {
    // Arrange
    let batches = run_script(
        "issue-796-power-operator-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 2 ^ 3 ^ 2 AS left_associative_power, \
                2 * 3 ^ 2 AS exponent_before_multiplication",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("power operator RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("64".to_owned()), Some("18".to_owned())]]
    );
}

#[test]
fn should_return_pg_input_error_info_as_a_star_expanded_row() {
    // Arrange
    let batches = run_script(
        "issue-796-pg-input-error-info-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT * FROM pg_input_error_info('not-an-int', 'integer')",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    let errors = result
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(
        wire::error_code(result),
        None,
        "pg_input_error_info ErrorResponse: {errors:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("pg_input_error_info RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["message", "detail", "hint", "sql_error_code"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [25, 25, 25, 25]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("invalid input syntax for type integer: \"not-an-int\"".to_owned()),
            None,
            None,
            Some("22P02".to_owned()),
        ]]
    );
}

#[test]
fn should_parse_postgres_nested_block_comments_beyond_one_hundred_twenty_eight_levels() {
    // Arrange
    let comment = format!("{}nested{}", "/*".repeat(129), "*/".repeat(129));
    let sql = format!("{comment} SELECT 7::INT4 AS parsed_value");
    let batches = run_script(
        "issue-796-nested-block-comment-depth",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(&sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    let errors = result
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(
        wire::error_code(result),
        None,
        "nested block comment ErrorResponse: {errors:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("nested-comment query RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns[0].name, "parsed_value");
    assert_eq!(columns[0].type_oid, 23);
    assert_eq!(wire::data_rows(result), vec![vec![Some("7".to_owned())]]);
}

#[test]
fn should_select_postgres_integer_literal_oids_at_the_signed_boundary() {
    // Arrange
    let batches = run_script(
        "issue-796-integer-literal-width-boundary",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 2147483647 AS int4_max, 2147483648 AS int8_min",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    let errors = result
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(
        wire::error_code(result),
        None,
        "integer literal boundary ErrorResponse: {errors:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("integer literal boundary RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        [("int4_max", 23), ("int8_min", 20)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("2147483647".to_owned()),
            Some("2147483648".to_owned()),
        ]]
    );
}
