#[test]
fn should_bind_split_part_parameters_with_text_descriptor() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "split_part_parameters",
            "SELECT split_part($1, $2, $3)",
            &[25, 25, 23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("split_part_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "split_part_portal",
                "split_part_parameters",
                &["one,two,three", ",", "2"],
            ),
            wire::describe_portal_frame("split_part_portal"),
            wire::execute_frame("split_part_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-split-part-wire", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let parameter_types = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![25, 25, 23]
    );
    let result_columns = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_columns.1)[0].type_oid,
        25
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("two".to_owned())]]
    );
}

#[test]
fn should_preserve_left_text_parameter_wire_metadata() {
    // Arrange
    let sql = "SELECT left($1, $2) AS prefix";
    let text_bytes = b"abcdef";
    let count_bytes = 3_i32.to_be_bytes();
    let batches = run_script(
        "issue-803-left-parameter-wire-metadata",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types("left_text", sql, &[0, 0])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("left_text"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "left_binary",
                    "left_text",
                    &[1, 1],
                    &[Some(text_bytes), Some(&count_bytes)],
                    &[1],
                ),
                wire::describe_portal_frame("left_binary"),
                wire::execute_frame("left_binary"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("ParameterDescription")
                .1
        ),
        vec![25, 23]
    );
    let described_columns = wire::parse_row_description(
        &described
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("statement RowDescription")
            .1,
    );
    assert_eq!(described_columns[0].name, "prefix");
    assert_eq!(described_columns[0].type_oid, 25);
    let executed_columns = wire::parse_row_description(
        &executed
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("portal RowDescription")
            .1,
    );
    assert_eq!(executed_columns[0].format_code, 1);
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("abc".to_owned())]]
    );
}

#[test]
fn should_bind_string_to_table_parameters_with_postgres_result_metadata() {
    // Arrange
    let sql = "SELECT value, ordinal FROM string_to_table($1, $2, $3) WITH ORDINALITY AS t(value, ordinal) ORDER BY ordinal";
    let batches = run_script(
        "issue-803-string-to-table-parameters",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "string_to_table_parameters",
                sql,
                &[25, 25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("string_to_table_parameters"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "string_to_table_portal",
                    "string_to_table_parameters",
                    &["a,NULL,b", ",", "NULL"],
                ),
                wire::describe_portal_frame("string_to_table_portal"),
                wire::execute_frame("string_to_table_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("string_to_table ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 25, 25]
    );
    let columns = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("string_to_table portal RowDescription");
    assert_eq!(
        wire::parse_row_description(&columns.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 20]
    );
    assert_eq!(
        wire::data_rows(executed),
        vec![
            vec![Some("a".to_owned()), Some("1".to_owned())],
            vec![None, Some("2".to_owned())],
            vec![Some("b".to_owned()), Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_bind_ascii_name_plus_integer_overloads_after_describe() {
    // Arrange
    let batches = run_script(
        "issue-803-to-ascii-overload-wire-metadata",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "to_ascii_name",
                "SELECT to_ascii($1, $2) AS converted",
                &[25, 19],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("to_ascii_name"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("to_ascii_name_portal", "to_ascii_name", &["abc", "LATIN1"]),
                wire::describe_portal_frame("to_ascii_name_portal"),
                wire::execute_frame("to_ascii_name_portal"),
                wire::sync_frame(),
            ]),
            Step::ParseFlush(wire::parse_frame_with_types(
                "to_ascii_integer",
                "SELECT to_ascii($1, $2) AS converted",
                &[25, 23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("to_ascii_integer"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("to_ascii_integer_portal", "to_ascii_integer", &["abc", "8"]),
                wire::describe_portal_frame("to_ascii_integer_portal"),
                wire::execute_frame("to_ascii_integer_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let name_description = &batches[1];
    let name_result = &batches[2];
    let integer_description = &batches[4];
    let integer_result = &batches[5];

    // Assert
    for (parsed, described, executed, parameter_oids) in [
        (&batches[0], name_description, name_result, vec![25, 19]),
        (
            &batches[3],
            integer_description,
            integer_result,
            vec![25, 23],
        ),
    ] {
        assert_eq!(wire::error_code(parsed), None);
        assert_eq!(wire::error_code(described), None);
        assert_eq!(wire::error_code(executed), None);
        let parameters = described
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("to_ascii ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            parameter_oids
        );
        let columns = executed
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("to_ascii portal RowDescription");
        assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 25);
        assert_eq!(
            wire::data_rows(executed),
            vec![vec![Some("abc".to_owned())]]
        );
    }
}
