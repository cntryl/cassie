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
        &[
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), Some("2".to_owned())],
        ],
        "same-source plain JOIN control",
    );
    assert_subquery_result(
        &batches[5],
        &[23, 23],
        &[
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
        &[
            vec![Some("first".to_owned()), Some("1".to_owned())],
            vec![Some("second".to_owned()), Some("2".to_owned())],
        ],
        "same-source plain ORDER BY control",
    );
    assert_subquery_result(
        &batches[3],
        &[25, 23],
        &[
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
        &[vec![Some("2".to_owned()), Some("1".to_owned())]],
        "same-source plain HAVING control",
    );
    assert_subquery_result(
        &batches[3],
        &[23, 23],
        &[vec![Some("2".to_owned()), Some("1".to_owned())]],
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
        &[vec![Some("300".to_owned())]],
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
        &[
            vec![Some("4".to_owned()), Some("2".to_owned())],
            vec![Some("5".to_owned()), Some("1".to_owned())],
        ],
        "same-source plain GROUP BY control",
    );
    assert_subquery_result(
        &batches[3],
        &[23, 23],
        &[
            vec![Some("4".to_owned()), Some("2".to_owned())],
            vec![Some("5".to_owned()), Some("1".to_owned())],
        ],
        "GROUP BY correlated scalar subquery",
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
        &[vec![Some("7".to_owned())]],
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
        &[vec![Some("9".to_owned())]],
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
