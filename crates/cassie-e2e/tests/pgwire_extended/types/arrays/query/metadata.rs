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
        vec![23, 1_007]
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
        [("chained", 25), ("array_value", 1_007)]
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
        vec![25, 1_007]
    );
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("42".to_owned()), Some("{1,2}".to_owned())]]
    );
}
