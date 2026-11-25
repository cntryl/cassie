use super::*;

#[test]
fn should_infer_format_argument_types_for_oid_zero_parameters() {
    // Arrange
    let statement = "format_oid_zero_parameters";
    let portal = "format_oid_zero_parameters_portal";
    let sql = "SELECT format('%I|%L|%s', $1::TEXT, $2::TEXT, $3::TEXT) AS formatted";
    let batches = run_script(
        "issue-803-format-oid-zero-parameters",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[0, 0, 0])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(portal, statement, &["Foo bar", "O'Reilly", "42"]),
                wire::describe_portal_frame(portal),
                wire::execute_frame(portal),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let result = &batches[2];

    // Assert
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 25, 25]
    );
    let result_columns = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_columns.1)[0].type_oid,
        25
    );
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r#""Foo bar"|'O''Reilly'|42"#.to_owned())]]
    );
}

#[test]
fn should_format_binary_bound_arguments_with_text_binary_result() {
    // Arrange
    let statement = "format_binary_parameters";
    let portal = "format_binary_parameters_portal";
    let sql = "SELECT format('%I|%L|%s', $1::TEXT, $2::TEXT, $3::TEXT) AS formatted";
    let batches = run_script(
        "issue-803-format-binary-parameters",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[25, 25, 25])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    portal,
                    statement,
                    &[1],
                    &[Some(b"Foo bar"), Some(b"O'Reilly"), Some(b"42")],
                    &[1],
                ),
                wire::describe_portal_frame(portal),
                wire::execute_frame(portal),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let result = &batches[2];

    // Assert
    assert_eq!(wire::error_code(described), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("ParameterDescription")
                .1
        ),
        vec![25, 25, 25]
    );
    assert_eq!(wire::error_code(result), None);
    let result_description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary format RowDescription");
    let columns = wire::parse_row_description(&result_description.1);
    assert_eq!(columns.len(), 1);
    assert_eq!((columns[0].type_oid, columns[0].format_code), (25, 1));
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r#""Foo bar"|'O''Reilly'|42"#.to_owned())]]
    );
}
