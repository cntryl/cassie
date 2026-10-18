#[test]
fn should_expose_the_builtin_postgres_collation_catalog_rows() {
    // Arrange
    let batches = run_script(
        "issue-796-pg-collation-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT collname, collprovider, collisdeterministic FROM pg_collation WHERE collname = 'C'",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        wire::row_description_names(result),
        vec!["collname", "collprovider", "collisdeterministic"]
    );
    assert_eq!(wire::data_rows(result).len(), 1);
    assert_eq!(wire::data_rows(result)[0][0].as_deref(), Some("C"));
}

#[test]
fn should_describe_values_relation_columns_over_pgwire() {
    // Arrange
    let batches = run_script(
        "issue-800-values-row-wire-metadata",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT * FROM (VALUES (1::BIGINT, 'alpha'::TEXT), (NULL::BIGINT, 'beta'::TEXT)) AS v(id, label)",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    let error_fields = result
        .iter()
        .find(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1));
    assert_eq!(
        wire::error_code(result),
        None,
        "valid VALUES relation failed: {error_fields:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("typed VALUES RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "label"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [20, 25]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("1".to_owned()), Some("alpha".to_owned())],
            vec![None, Some("beta".to_owned())],
        ]
    );
}
