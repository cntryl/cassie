//! Residual PostgreSQL 18 scalar and row-subquery contracts for #762.

use std::sync::Arc;
use std::time::Duration;

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture_with_config;
use cassie::config::CassieRuntimeConfig;

fn assert_subquery_result(
    frames: &[(u8, Vec<u8>)],
    expected_oids: &[i32],
    expected_rows: Vec<Vec<Option<String>>>,
    context: &str,
) {
    let errors = frames
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(
        wire::error_code(frames),
        None,
        "{context} ErrorResponse: {errors:?}"
    );
    let description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .unwrap_or_else(|| panic!("{context} must return RowDescription"));
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids,
        "{context} result type OIDs"
    );
    assert_eq!(wire::data_rows(frames), expected_rows, "{context} values");
}

#[test]
fn should_accept_scalar_subqueries_in_join_conditions() {
    // Arrange
    let control_sql = "SELECT left_row.id, right_row.id \
                      FROM issue_762_join_left AS left_row \
                      JOIN issue_762_join_right AS right_row \
                        ON left_row.id = right_row.id \
                      ORDER BY left_row.id, right_row.id";
    let sql = "SELECT left_row.id, right_row.id \
               FROM issue_762_join_left AS left_row \
               JOIN issue_762_join_right AS right_row \
                 ON left_row.id = (SELECT right_row.id) \
               ORDER BY left_row.id, right_row.id";

    // Act
    let batches = run_script(
        "issue-762-scalar-subquery-join-condition",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_join_left (id INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_join_right (id INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_join_left VALUES (1), (2)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_join_right VALUES (1), (2)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(control_sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    );

    // Assert
    assert_subquery_result(
        &batches[4],
        &[23, 23],
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), Some("2".to_owned())],
        ],
        "same-source plain JOIN control",
    );
    assert_subquery_result(
        &batches[5],
        &[23, 23],
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), Some("2".to_owned())],
        ],
        "JOIN condition scalar subquery",
    );
}

#[test]
fn should_accept_scalar_subqueries_in_order_by_expressions() {
    // Arrange
    let control_sql = "SELECT ordered_values.label, ordered_values.sort_key \
                      FROM issue_762_order_values AS ordered_values \
                      ORDER BY ordered_values.sort_key";
    let sql = "SELECT ordered_values.label, ordered_values.sort_key \
               FROM issue_762_order_values AS ordered_values \
               ORDER BY (SELECT ordered_values.sort_key)";

    // Act
    let batches = run_script(
        "issue-762-scalar-subquery-order-by",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_order_values (label TEXT, sort_key INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_order_values VALUES ('second', 2), ('first', 1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(control_sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    );

    // Assert
    assert_subquery_result(
        &batches[2],
        &[25, 23],
        vec![
            vec![Some("first".to_owned()), Some("1".to_owned())],
            vec![Some("second".to_owned()), Some("2".to_owned())],
        ],
        "same-source plain ORDER BY control",
    );
    assert_subquery_result(
        &batches[3],
        &[25, 23],
        vec![
            vec![Some("first".to_owned()), Some("1".to_owned())],
            vec![Some("second".to_owned()), Some("2".to_owned())],
        ],
        "ORDER BY scalar subquery",
    );
}

#[test]
fn should_accept_correlated_scalar_subqueries_in_having() {
    // Arrange
    let control_sql = "SELECT grouped_values.bucket, COUNT(*)::INT4 AS row_count \
                      FROM issue_762_having_values AS grouped_values \
                      GROUP BY grouped_values.bucket \
                      HAVING grouped_values.bucket = 2 \
                      ORDER BY grouped_values.bucket";
    let sql = "SELECT grouped_values.bucket, COUNT(*)::INT4 AS row_count \
               FROM issue_762_having_values AS grouped_values \
               GROUP BY grouped_values.bucket \
               HAVING (SELECT grouped_values.bucket) = 2 \
               ORDER BY grouped_values.bucket";

    // Act
    let batches = run_script(
        "issue-762-correlated-scalar-subquery-having",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_having_values (bucket INT4, value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_having_values VALUES (1, 5), (1, 6), (2, 9)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(control_sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    );

    // Assert
    assert_subquery_result(
        &batches[2],
        &[23, 23],
        vec![vec![Some("2".to_owned()), Some("1".to_owned())]],
        "same-source plain HAVING control",
    );
    assert_subquery_result(
        &batches[3],
        &[23, 23],
        vec![vec![Some("2".to_owned()), Some("1".to_owned())]],
        "HAVING correlated scalar subquery",
    );
}

#[test]
fn should_aggregate_correlated_scalar_subquery_results() {
    // Arrange
    let control_sql = "SELECT SUM(value) AS total FROM scalar_wire_values";
    let sql = "SELECT SUM((SELECT inner_row.value FROM scalar_wire_values AS inner_row \
                   WHERE inner_row.id = outer_row.id)) AS total \
               FROM scalar_wire_values AS outer_row";

    // Act
    let batches = run_script(
        "issue-762-correlated-scalar-subquery-in-aggregate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE scalar_wire_values (id INT, value INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO scalar_wire_values (id, value) VALUES (1, 100), (2, 200)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(control_sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    );
    let control = &batches[2];
    let result = &batches[3];

    // Assert
    assert_subquery_result(
        control,
        &[20],
        vec![vec![Some("300".to_owned())]],
        "same-source plain aggregate control",
    );
    assert_eq!(
        wire::error_code(result),
        None,
        "correlated scalar-subquery aggregate ErrorResponse: {:?}",
        result
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("aggregate scalar-subquery RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 20);
    assert_eq!(wire::data_rows(result), vec![vec![Some("300".to_owned())]]);
}

#[test]
fn should_group_by_a_correlated_scalar_subquery_expression() {
    // Arrange
    let create_table = "CREATE TABLE issue_762_group_by_source (value INT4)";
    let insert_rows = "INSERT INTO issue_762_group_by_source (value) VALUES (4), (4), (5)";
    let control_sql = "SELECT grouped_row.value::INT4 AS grouped_value, \
                      COUNT(*)::INT4 AS row_count \
                      FROM issue_762_group_by_source AS grouped_row \
                      GROUP BY grouped_row.value \
                      ORDER BY grouped_value";
    let sql = "SELECT (SELECT grouped_row.value)::INT4 AS grouped_value, \
               COUNT(*)::INT4 AS row_count \
               FROM issue_762_group_by_source AS grouped_row \
               GROUP BY (SELECT grouped_row.value) \
               ORDER BY grouped_value";

    // Act
    let batches = run_script(
        "issue-762-correlated-scalar-subquery-group-by",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(create_table)]),
            Step::ThroughReady(vec![wire::simple_query_frame(insert_rows)]),
            Step::ThroughReady(vec![wire::simple_query_frame(control_sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    );

    // Assert
    assert_subquery_result(
        &batches[2],
        &[23, 23],
        vec![
            vec![Some("4".to_owned()), Some("2".to_owned())],
            vec![Some("5".to_owned()), Some("1".to_owned())],
        ],
        "same-source plain GROUP BY control",
    );
    assert_subquery_result(
        &batches[3],
        &[23, 23],
        vec![
            vec![Some("4".to_owned()), Some("2".to_owned())],
            vec![Some("5".to_owned()), Some("1".to_owned())],
        ],
        "GROUP BY correlated scalar subquery",
    );
}

#[test]
fn should_preserve_bigint_parameter_identity_through_binary_scalar_execution() {
    // Arrange
    let control_statement = "bigint_parameter_control";
    let target_statement = "scalar_subquery_bigint";
    let control_portal = "bigint_parameter_control_binary";
    let target_portal = "scalar_subquery_bigint_binary";
    let high_value = 9_007_199_254_740_993_i64.to_be_bytes();
    let batches = run_script(
        "issue-762-scalar-subquery-bigint-binary-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                control_statement,
                "SELECT $1::BIGINT AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(control_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    control_portal,
                    control_statement,
                    &[1],
                    &[Some(&high_value)],
                    &[1],
                ),
                wire::describe_portal_frame(control_portal),
                wire::execute_frame(control_portal),
                wire::sync_frame(),
            ]),
            Step::ParseFlush(wire::parse_frame_with_types(
                target_statement,
                "SELECT (SELECT $1::BIGINT) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(target_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    target_portal,
                    target_statement,
                    &[1],
                    &[Some(&high_value)],
                    &[1],
                ),
                wire::describe_portal_frame(target_portal),
                wire::execute_frame(target_portal),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let control_parsed = &batches[0];
    let control_described = &batches[1];
    let control_executed = &batches[2];
    let parsed = &batches[3];
    let described = &batches[4];
    let executed = &batches[5];

    // Assert
    assert_eq!(wire::error_code(control_parsed), None);
    assert_eq!(wire::error_code(control_described), None);
    assert_eq!(wire::error_code(control_executed), None);
    assert_eq!(
        wire::parse_parameter_description(
            &control_described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("direct BIGINT ParameterDescription")
                .1
        ),
        vec![20]
    );
    let control_portal_description = control_executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("direct BIGINT binary portal RowDescription");
    assert_eq!(
        (
            wire::parse_row_description(&control_portal_description.1)[0].type_oid,
            wire::parse_row_description(&control_portal_description.1)[0].format_code
        ),
        (20, 1)
    );
    let control_data_row = control_executed
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("direct BIGINT binary DataRow");
    assert_eq!(&control_data_row.1[6..], &high_value);

    for (stage, frames) in [
        ("Parse", parsed),
        ("Describe", described),
        ("binary Bind/Execute", executed),
    ] {
        let errors = frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>();
        assert_eq!(
            wire::error_code(frames),
            None,
            "scalar BIGINT {stage} ErrorResponse: {errors:?}"
        );
    }
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("scalar BIGINT ParameterDescription")
                .1
        ),
        vec![20]
    );
    assert_eq!(
        wire::parse_row_description(
            &described
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("scalar BIGINT statement RowDescription")
                .1
        )[0]
        .type_oid,
        20
    );
    let portal_description = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary scalar BIGINT portal RowDescription");
    let portal_columns = wire::parse_row_description(&portal_description.1);
    let portal_column = &portal_columns[0];
    assert_eq!((portal_column.type_oid, portal_column.format_code), (20, 1));
    let data_row = executed
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary scalar BIGINT DataRow");
    assert_eq!(&data_row.1[..2], &1_i16.to_be_bytes());
    assert_eq!(&data_row.1[2..6], &8_i32.to_be_bytes());
    assert_eq!(&data_row.1[6..], &high_value);
}

#[test]
fn should_match_quantified_comparison_operator_truth_tables() {
    // Arrange
    let operators = ["=", "<>", "<", "<=", ">", ">="];
    let scenarios = [
        (
            "non_null",
            "scenario = 'non_null'",
            [
                Some("f"),
                Some("t"),
                Some("t"),
                Some("t"),
                Some("t"),
                Some("t"),
            ],
            [
                Some("f"),
                Some("t"),
                Some("f"),
                Some("f"),
                Some("f"),
                Some("f"),
            ],
        ),
        (
            "with_null",
            "scenario = 'with_null'",
            [None, Some("t"), Some("t"), Some("t"), Some("t"), Some("t")],
            [Some("f"), None, Some("f"), Some("f"), Some("f"), Some("f")],
        ),
        ("empty", "false", [Some("f"); 6], [Some("t"); 6]),
    ];
    let mut cases = Vec::new();
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_762_quantifier_values (scenario TEXT, value INT4)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_762_quantifier_values VALUES \
             ('non_null', 1), ('non_null', 3), \
             ('with_null', 1), ('with_null', 3), ('with_null', NULL)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT value FROM issue_762_quantifier_values \
             WHERE scenario = 'non_null' ORDER BY value",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 2 < ANY (SELECT value FROM issue_762_quantifier_values \
             WHERE scenario = 'non_null') AS scalar_any_control",
        )]),
    ];
    for (scenario, predicate, any_results, all_results) in scenarios {
        for (quantifier, results) in [("ANY", any_results), ("ALL", all_results)] {
            for (index, operator) in operators.iter().enumerate() {
                let sql = format!(
                    "SELECT 2 {operator} {quantifier} \
                     (SELECT value FROM issue_762_quantifier_values WHERE {predicate}) \
                     AS comparison_result"
                );
                cases.push((scenario, quantifier, *operator, sql, results[index]));
                steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                    cases.last().expect("just-added comparison case").3.as_str(),
                )]));
            }
        }
    }

    // Act
    let batches = run_script("issue-762-any-all-operator-truth-tables", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() + 4);
    assert_subquery_result(
        &batches[2],
        &[23],
        vec![vec![Some("1".to_owned())], vec![Some("3".to_owned())]],
        "same-source quantifier table control",
    );
    assert_subquery_result(
        &batches[3],
        &[16],
        vec![vec![Some("t".to_owned())]],
        "same-source scalar ANY control",
    );
    for ((scenario, quantifier, operator, sql, expected), result) in cases.iter().zip(&batches[4..])
    {
        assert_eq!(
            wire::error_code(result),
            None,
            "unexpected error for {scenario} {quantifier} {operator}: {sql}; ErrorResponse: {:?}",
            result
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
        let description = result
            .iter()
            .find(|frame| frame.0 == b'T')
            .unwrap_or_else(|| panic!("missing Boolean RowDescription for {sql}"));
        assert_eq!(
            wire::parse_row_description(&description.1)[0].type_oid,
            16,
            "comparison result type for {sql}"
        );
        assert_eq!(
            wire::data_rows(result),
            vec![vec![expected.map(str::to_owned)]],
            "PostgreSQL 18.6 truth value for {sql}"
        );
    }
}

#[test]
fn should_preserve_correlated_row_subquery_null_truth_values() {
    // Arrange
    let control_sql = "SELECT o.id, ROW(o.a, o.b) = ROW(i.x, i.y) AS row_equal \
                       FROM issue_762_row_outer AS o \
                       LEFT JOIN issue_762_row_inner AS i ON i.parent_id = o.id \
                       ORDER BY o.id";
    let sql = "SELECT o.id,\
                      ROW(o.a, o.b) = (SELECT i.x, i.y FROM issue_762_row_inner AS i\
                                       WHERE i.parent_id = o.id) AS row_equal,\
                      ROW(o.a, o.b) IN (SELECT i.x, i.y FROM issue_762_row_inner AS i\
                                       WHERE i.parent_id = o.id) AS row_in,\
                      ROW(o.a, o.b) NOT IN (SELECT i.x, i.y FROM issue_762_row_inner AS i\
                                           WHERE i.parent_id = o.id) AS row_not_in\
               FROM issue_762_row_outer AS o ORDER BY o.id";
    let batches = run_script(
        "issue-762-correlated-row-subquery-truth-table",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_row_outer (id INT4, a INT4, b INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_row_inner (parent_id INT4, x INT4, y INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_row_outer VALUES \
                 (1, 1, 10), (2, 2, 20), (3, 3, 30), (4, 4, 40)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_row_inner VALUES \
                 (1, 1, 10), (2, 2, NULL), (4, 4, 41)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(control_sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    );

    // Act
    let control = &batches[4];
    let result = &batches[5];

    // Assert
    assert_subquery_result(
        control,
        &[23, 16],
        vec![
            vec![Some("1".to_owned()), Some("t".to_owned())],
            vec![Some("2".to_owned()), None],
            vec![Some("3".to_owned()), None],
            vec![Some("4".to_owned()), Some("f".to_owned())],
        ],
        "plain-table row-comparison control",
    );
    assert_eq!(
        wire::error_code(result),
        None,
        "correlated row-subquery ErrorResponse: {:?}",
        result
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("correlated row-subquery RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 16, 16, 16]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "row_equal", "row_in", "row_not_in"]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("1".to_owned()),
                Some("t".to_owned()),
                Some("t".to_owned()),
                Some("f".to_owned())
            ],
            vec![Some("2".to_owned()), None, None, None],
            vec![
                Some("3".to_owned()),
                None,
                Some("f".to_owned()),
                Some("t".to_owned())
            ],
            vec![
                Some("4".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned()),
                Some("t".to_owned())
            ],
        ]
    );
}

#[test]
fn should_execute_binary_row_subquery_with_described_metadata() {
    // Arrange
    let control_statement = "row_constructor_control";
    let target_statement = "row_subquery";
    let control_portal = "row_constructor_control_binary";
    let target_portal = "row_subquery_binary";
    let control_sql = "SELECT ROW($1::INT4, $2::INT4) = \
                       ROW(1::INT4, 10::INT4) AS matches";
    let sql = "SELECT ROW($1::INT4, $2::INT4) = (\
                   SELECT candidate.x, candidate.y \
                   FROM issue_762_binary_row_values AS candidate \
               ) AS matches";
    let first_value = 1_i32.to_be_bytes();
    let second_value = 10_i32.to_be_bytes();
    let batches = run_script(
        "issue-762-row-subquery-binary-parameters",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_binary_row_values (x INT4, y INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_binary_row_values VALUES (1, 10)",
            )]),
            Step::ParseFlush(wire::parse_frame_with_types(
                control_statement,
                control_sql,
                &[0, 0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(control_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    control_portal,
                    control_statement,
                    &[1],
                    &[Some(&first_value), Some(&second_value)],
                    &[1],
                ),
                wire::describe_portal_frame(control_portal),
                wire::execute_frame(control_portal),
                wire::sync_frame(),
            ]),
            Step::ParseFlush(wire::parse_frame_with_types(target_statement, sql, &[0, 0])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(target_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    target_portal,
                    target_statement,
                    &[1],
                    &[Some(&first_value), Some(&second_value)],
                    &[1],
                ),
                wire::describe_portal_frame(target_portal),
                wire::execute_frame(target_portal),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let control_parsed = &batches[2];
    let control_described = &batches[3];
    let control_executed = &batches[4];
    let parsed = &batches[5];
    let described = &batches[6];
    let executed = &batches[7];

    // Assert
    assert!(batches[..2]
        .iter()
        .all(|frames| wire::error_code(frames).is_none()));
    assert_eq!(wire::error_code(control_parsed), None);
    assert_eq!(wire::error_code(control_described), None);
    assert_eq!(wire::error_code(control_executed), None);
    assert_eq!(
        wire::parse_parameter_description(
            &control_described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("row-constructor control ParameterDescription")
                .1
        ),
        vec![23, 23]
    );
    assert_eq!(
        wire::parse_row_description(
            &control_described
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("row-constructor control RowDescription")
                .1
        )[0]
        .type_oid,
        16
    );
    let control_data = control_executed
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary row-constructor control DataRow");
    assert_eq!(&control_data.1[6..], &[1]);

    assert_eq!(
        wire::error_code(parsed),
        None,
        "row-subquery Parse ErrorResponse: {:?}",
        parsed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(described),
        None,
        "row-subquery Describe ErrorResponse: {:?}",
        described
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("row-subquery ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![23, 23]
    );
    let statement_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-subquery statement RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    assert_eq!(statement_columns[0].name, "matches");
    assert_eq!(statement_columns[0].type_oid, 16);

    assert_eq!(
        wire::error_code(executed),
        None,
        "binary row-subquery Execute ErrorResponse: {:?}",
        executed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let portal_description = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary row-subquery portal RowDescription");
    let portal_column = &wire::parse_row_description(&portal_description.1)[0];
    assert_eq!(portal_column.type_oid, 16);
    assert_eq!(portal_column.format_code, 1);
    let data_row = executed
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary row-subquery DataRow");
    assert_eq!(
        i16::from_be_bytes(data_row.1[0..2].try_into().expect("field count")),
        1
    );
    assert_eq!(
        i32::from_be_bytes(data_row.1[2..6].try_into().expect("field length")),
        1
    );
    assert_eq!(data_row.1[6], 1);
}

#[test]
fn should_project_scalar_subqueries_within_the_configured_result_row_limit() {
    // Arrange
    wire::use_local_storage();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 2;
    let storage = sql_fixture_with_config("issue-762-scalar-result-limit", &[], config.clone());
    storage
        .cassie
        .startup()
        .expect("start scalar result-limit fixture");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let (setup, baseline, within_limit, overflow, recovery) = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            Arc::new(storage.cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup(&mut reader, &mut writer).await;

        let mut setup = Vec::new();
        for statement in [
            "CREATE TABLE issue_762_cap_outer (id INT4 PRIMARY KEY)",
            "CREATE TABLE issue_762_cap_inner (id INT4, value INT4)",
            "INSERT INTO issue_762_cap_outer VALUES (1), (2), (3)",
            "INSERT INTO issue_762_cap_inner VALUES (1, 10), (2, 20), (3, 30)",
        ] {
            wire::write_frames(
                &mut writer,
                vec![wire::simple_query_frame(statement)],
            )
            .await;
            setup.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT o.id, o.id * 10 AS scalar_value \
                 FROM issue_762_cap_outer o WHERE o.id <= 2 ORDER BY o.id",
            )],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT o.id, (SELECT i.value FROM issue_762_cap_inner i WHERE i.id = o.id) AS scalar_value \
                 FROM issue_762_cap_outer o WHERE o.id <= 2 ORDER BY o.id",
            )],
        )
        .await;
        let within_limit =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT o.id, o.id * 10 AS scalar_value \
                 FROM issue_762_cap_outer o ORDER BY o.id",
            )],
        )
        .await;
        let overflow =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 46 AS recovery")])
            .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (setup, baseline, within_limit, overflow, recovery)
    });
    drop(runtime);

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "scalar result-limit setup ErrorResponses: {:?}",
        setup
            .iter()
            .flat_map(|frames| frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(&baseline),
        None,
        "plain projection limit control ErrorResponse: {:?}",
        baseline
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(&baseline),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
        ]
    );
    assert_eq!(
        wire::error_code(&within_limit),
        None,
        "within-limit scalar-subquery ErrorResponse: {:?}",
        within_limit
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(&within_limit),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
        ]
    );
    assert_eq!(
        wire::error_code(&overflow).as_deref(),
        Some("54000"),
        "plain projection overflow control ErrorResponse: {:?}",
        overflow
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert!(wire::data_rows(&overflow).is_empty());
    assert_eq!(overflow.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(overflow.last().map(|frame| frame.0), Some(b'Z'));
    let error = overflow
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("scalar result-limit ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M' && message == "query result row limit exceeded: 3 > 2"
    }));
    assert_eq!(
        wire::error_code(&recovery),
        None,
        "post-limit recovery ErrorResponse: {:?}",
        recovery
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("46".to_owned())]]
    );
}

#[test]
fn should_accept_scalar_subquery_in_case_expression() {
    // Arrange
    let batches = run_script(
        "issue-762-case-scalar-subquery-short-circuit",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_unselected_scalar_error (denominator INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_unselected_scalar_error VALUES (0)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT CASE WHEN FALSE THEN 0 ELSE 7 END AS case_value",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT CASE WHEN FALSE THEN \
                    (SELECT 10 / (denominator - denominator) \
                     FROM issue_762_unselected_scalar_error) \
                 ELSE 7 END AS case_value",
            )]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let control = &batches[2];
    let case_result = &batches[3];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "scalar-subquery setup errors: {:?}",
        setup
            .iter()
            .flat_map(|frames| frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1)))
            .collect::<Vec<_>>()
    );
    assert_subquery_result(
        control,
        &[23],
        vec![vec![Some("7".to_owned())]],
        "plain CASE short-circuit control",
    );
    assert_eq!(
        wire::error_code(case_result),
        None,
        "CASE scalar-subquery error: {:?}",
        case_result
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let case_description = case_result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("CASE scalar-subquery RowDescription");
    assert_eq!(
        wire::parse_row_description(&case_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23]
    );
    assert_eq!(wire::data_rows(case_result), vec![vec![Some("7".into())]]);
}

#[test]
fn should_accept_scalar_subquery_in_coalesce_expression() {
    // Arrange
    let batches = run_script(
        "issue-762-coalesce-scalar-subquery-short-circuit",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_unselected_coalesce_error (denominator INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_unselected_coalesce_error VALUES (0)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT COALESCE(9, 10) AS coalesce_value",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT COALESCE(9, \
                    (SELECT 10 / (denominator - denominator) \
                     FROM issue_762_unselected_coalesce_error)) AS coalesce_value",
            )]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let control = &batches[2];
    let coalesce_result = &batches[3];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "scalar-subquery setup errors: {:?}",
        setup
            .iter()
            .flat_map(|frames| frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1)))
            .collect::<Vec<_>>()
    );
    assert_subquery_result(
        control,
        &[23],
        vec![vec![Some("9".to_owned())]],
        "plain COALESCE short-circuit control",
    );
    assert_eq!(
        wire::error_code(coalesce_result),
        None,
        "COALESCE scalar-subquery error: {:?}",
        coalesce_result
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let coalesce_description = coalesce_result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("COALESCE scalar-subquery RowDescription");
    assert_eq!(
        wire::parse_row_description(&coalesce_description.1)[0].type_oid,
        23
    );
    assert_eq!(
        wire::data_rows(coalesce_result),
        vec![vec![Some("9".into())]]
    );
}
