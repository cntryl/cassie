#[test]
fn should_describe_postgres_cast_parameter_oid_through_extended_protocol() {
    // Arrange
    let batches = run_script(
        "issue-796-cast-parameter-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "cast_parameter",
                "SELECT $1::DOUBLE PRECISION AS value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("cast_parameter"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(described), None);
    let parameter_types = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![701]
    );
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 701);
}

#[test]
fn should_bind_the_selected_four_type_cast_fixture_through_extended_protocol() {
    // Arrange
    let sql = "SELECT $1::uuid AS uuid_value, $2::jsonb AS document, \
        $3::numeric(18,2) AS amount, $4::uuid[] AS ids";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "selected_type_casts",
            sql,
            &[0, 0, 0, 0],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("selected_type_casts"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "selected_type_casts_values",
                "selected_type_casts",
                &[
                    "550E8400-E29B-41D4-A716-446655440000",
                    r#"{"answer":42}"#,
                    "12.345",
                    "{550E8400-E29B-41D4-A716-446655440001,NULL}",
                ],
            ),
            wire::describe_portal_frame("selected_type_casts_values"),
            wire::execute_frame("selected_type_casts_values"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "selected_type_casts_nulls",
                "selected_type_casts",
                &[0],
                &[None, None, None, None],
                &[],
            ),
            wire::describe_portal_frame("selected_type_casts_nulls"),
            wire::execute_frame("selected_type_casts_nulls"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-796-four-type-cast-parameters", steps);
    let described = &batches[1];
    let values = &batches[2];
    let nulls = &batches[3];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_selected_cast_statement_description(described);
    assert_selected_cast_bound_values(values);
    assert_selected_cast_null_values(nulls);
}

fn assert_selected_cast_statement_description(frames: &[(u8, Vec<u8>)]) {
    assert_eq!(wire::error_code(frames), None);
    let parameters = frames
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("inferred ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        [2950, 114, 1700, 36_950]
    );

    let statement_description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("cast statement RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    assert_eq!(
        statement_columns
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid, column.type_mod))
            .collect::<Vec<_>>(),
        [
            ("uuid_value", 2950, -1),
            ("document", 114, -1),
            ("amount", 1700, 1_179_654),
            ("ids", 36_950, -1),
        ]
    );
}

fn assert_selected_cast_bound_values(frames: &[(u8, Vec<u8>)]) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "bound cast ErrorResponse: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let portal_description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("bound portal RowDescription");
    assert_eq!(
        wire::parse_row_description(&portal_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [2950, 114, 1700, 36_950]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("550e8400-e29b-41d4-a716-446655440000".to_owned()),
            Some(r#"{"answer":42}"#.to_owned()),
            Some("12.35".to_owned()),
            Some("[\"550e8400-e29b-41d4-a716-446655440001\",null]".to_owned()),
        ]]
    );
}

fn assert_selected_cast_null_values(frames: &[(u8, Vec<u8>)]) {
    assert_eq!(wire::error_code(frames), None);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![None, None, None, None]]
    );
}

#[test]
fn should_bind_double_precision_table_columns_with_postgres_identity() {
    // Arrange
    let batches = run_script(
        "issue-796-double-precision-table-column",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_796_double_precision_rows (value DOUBLE PRECISION NOT NULL)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_796_double_precision_rows VALUES (1.25)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT value FROM issue_796_double_precision_rows",
            )]),
        ],
    );

    // Act
    let setup_errors = batches[..2]
        .iter()
        .filter_map(|batch| {
            let state = wire::error_code(batch)?;
            let fields = batch
                .iter()
                .find(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1));
            Some((state, fields))
        })
        .collect::<Vec<_>>();
    let result = &batches[2];

    // Assert
    assert!(
        setup_errors.is_empty(),
        "DOUBLE PRECISION table setup should succeed: {setup_errors:?}"
    );
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("DOUBLE PRECISION result RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 701);
    assert_eq!(wire::data_rows(result), vec![vec![Some("1.25".to_owned())]]);
}

#[test]
fn should_describe_parenthesized_parameter_arithmetic_in_offset() {
    // Arrange
    let batches = run_script(
        "issue-800-parenthesized-offset-arithmetic",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "good_offset",
                "SELECT 1 OFFSET ($1 + 1) ROWS",
                &[23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("good_offset"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![23]);
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 23);
}

#[test]
fn should_bind_parameterized_bernoulli_sampling_over_extended_protocol() {
    // Arrange
    let rows = (1..=128)
        .map(|id| format!("({id})"))
        .collect::<Vec<_>>()
        .join(", ");
    let insert = format!("INSERT INTO issue_800_sample_wire_rows VALUES {rows}");
    let sql = "SELECT id FROM issue_800_sample_wire_rows TABLESAMPLE BERNOULLI ($1) REPEATABLE ($2) ORDER BY id";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_800_sample_wire_rows (id INT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(&insert)]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "sample_parameters",
            sql,
            &[700, 701],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("sample_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("sample_portal", "sample_parameters", &["100", "0.5"]),
            wire::describe_portal_frame("sample_portal"),
            wire::execute_frame("sample_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-800-tablesample-parameter-wire", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        batches[2].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1'],
        "TABLESAMPLE Parse error fields: {:?}",
        batches[2]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::error_code(&batches[3]), None);
    let parameters = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("TABLESAMPLE ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![700, 701]
    );
    let columns = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("TABLESAMPLE result RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 23);
    assert_eq!(wire::error_code(&batches[4]), None);
    assert_eq!(
        wire::data_rows(&batches[4]),
        (1..=128)
            .map(|id| vec![Some(id.to_string())])
            .collect::<Vec<_>>()
    );
}

#[test]
fn should_bind_oid_zero_bigint_parameters_in_values_over_extended_protocol() {
    // Arrange
    let sql =
        "SELECT v.id FROM (VALUES ($1::BIGINT), ($2::BIGINT)) AS v(id) ORDER BY id NULLS FIRST";
    let high_value = 9_007_199_254_740_993_i64.to_be_bytes();
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "values_bigint_parameters",
            sql,
            &[0, 0],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("values_bigint_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "values_bigint_portal",
                "values_bigint_parameters",
                &[1],
                &[Some(&high_value), None],
                &[1],
            ),
            wire::describe_portal_frame("values_bigint_portal"),
            wire::execute_frame("values_bigint_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-800-values-parameter-wire", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "VALUES parameter Parse SQLSTATE; frames: {:?}",
        batches[0]
    );
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    let parameters = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("inferred VALUES ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![20, 20]
    );
    let statement_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("VALUES statement RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    assert_eq!(statement_columns[0].name, "id");
    assert_eq!(statement_columns[0].type_oid, 20);

    let bound = &batches[2];
    assert_eq!(wire::error_code(bound), None);
    let portal_description = bound
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary VALUES portal RowDescription");
    let portal_columns = wire::parse_row_description(&portal_description.1);
    assert_eq!(portal_columns[0].type_oid, 20);
    assert_eq!(portal_columns[0].format_code, 1);
    let data_rows = bound
        .iter()
        .filter(|(tag, _)| *tag == b'D')
        .map(|(_, payload)| payload.as_slice())
        .collect::<Vec<_>>();
    assert_eq!(
        data_rows,
        vec![
            &[0, 1, 255, 255, 255, 255][..],
            &[0, 1, 0, 0, 0, 8, 0, 32, 0, 0, 0, 0, 0, 1][..],
        ]
    );
}
