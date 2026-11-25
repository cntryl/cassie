#[test]
fn should_report_postgresql_math_result_oids_across_function_overloads() {
    // Arrange
    let statement = "math_numeric_float8_overloads";
    let portal = "math_numeric_float8_overloads_portal";
    let sql = "SELECT ROUND($1, $3) AS numeric_round, ROUND($2) AS float8_round, \
        TRUNC($1, $3) AS numeric_trunc, FLOOR($1) AS numeric_floor, \
        FLOOR($2) AS float8_floor, CEILING($1) AS numeric_ceiling, \
        CEILING($2) AS float8_ceiling";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            sql,
            &[1700, 701, 23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["1.25", "2.5", "1"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-math-result-identities", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(
        wire::error_code(described),
        None,
        "math overload Describe ErrorResponse fields: {:?}",
        described
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let parameter_types = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("math overload ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![1700, 701, 23]
    );
    let description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("math overload statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 701, 1700, 1700, 701, 1700, 701]
    );
    let result = &batches[2];
    assert_eq!(
        wire::error_code(result),
        None,
        "math overload Execute ErrorResponse fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("1.3".to_owned()),
            Some("2".to_owned()),
            Some("1.2".to_owned()),
            Some("1".to_owned()),
            Some("2".to_owned()),
            Some("2".to_owned()),
            Some("3".to_owned()),
        ]]
    );
}

#[test]
fn should_describe_untyped_ceiling_literal_as_numeric() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT CEILING(42.8) AS rounded_value",
    )])];

    // Act
    let batches = run_script("postgresql18-ceiling-untyped-numeric-descriptor", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("CEILING literal RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (1700, -1, -1)
    );
    assert_eq!(wire::data_rows(result), vec![vec![Some("43".to_owned())]]);
}
