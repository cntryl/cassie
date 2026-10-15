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
        &[
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
