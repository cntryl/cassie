#[test]
fn should_decode_temporal_infinity_text_parameters() {
    // Arrange
    let cases = [
        ("date_positive_infinity", 1_082, "infinity", "infinity"),
        ("date_negative_infinity", 1_082, "-infinity", "-infinity"),
        ("timestamp_positive_infinity", 1_114, "infinity", "infinity"),
        (
            "timestamp_negative_infinity",
            1_114,
            "-infinity",
            "-infinity",
        ),
        (
            "timestamptz_positive_infinity",
            1_184,
            "infinity",
            "infinity",
        ),
        (
            "timestamptz_negative_infinity",
            1_184,
            "-infinity",
            "-infinity",
        ),
    ];
    let mut steps = Vec::new();
    for (statement, oid, input, _) in &cases {
        let sql = "SELECT $1 AS value";
        let portal = format!("{statement}_portal");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            sql,
            &[*oid],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame(&portal, statement, &[*input]),
            wire::describe_portal_frame(&portal),
            wire::execute_frame(&portal),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("issue-814-temporal-infinity-text-input", steps);

    // Assert
    for (index, (statement, oid, _, expected)) in cases.into_iter().enumerate() {
        let result = &batches[index * 2 + 1];
        assert_eq!(
            wire::error_code(result),
            None,
            "{statement}: {:?}",
            result
                .iter()
                .filter(|(tag, _)| *tag == b'E')
                .map(|(_, payload)| wire::parse_error_fields(payload))
                .collect::<Vec<_>>()
        );
        let description = result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("temporal infinity RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!((column.type_oid, column.format_code), (oid, 0));
        assert_eq!(
            wire::data_rows(result),
            vec![vec![Some(expected.to_owned())]],
            "{statement} text value"
        );
    }
}

#[test]
fn should_accept_text_temporal_parameters_with_explicit_postgresql_oids() {
    // Arrange
    let sql = "SELECT $1 AS d, $2 AS t, $3 AS ts, $4 AS tz, $5 AS i, $6 AS tztime";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "temporal_text_parameters",
            sql,
            &[1082, 1083, 1114, 1184, 1186, 1266],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("temporal_text_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "temporal_text_portal",
                "temporal_text_parameters",
                &[
                    "2000-01-02",
                    "03:04:05.006007",
                    "2000-01-02 03:04:05.006007",
                    "2000-01-02 03:04:05.006007+00",
                    "1 mon 2 days 03:04:05.006007",
                    "03:04:05.006007+02",
                ],
            ),
            wire::describe_portal_frame("temporal_text_portal"),
            wire::execute_frame("temporal_text_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-text-parameters", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        wire::parse_parameter_description(
            &batches[1]
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("ParameterDescription")
                .1
        ),
        vec![1082, 1083, 1114, 1184, 1186, 1266]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("2000-01-02".to_owned()),
            Some("03:04:05.006007".to_owned()),
            Some("2000-01-02 03:04:05.006007".to_owned()),
            Some("2000-01-02 03:04:05.006007+00".to_owned()),
            Some("1 mon 2 days 03:04:05.006007".to_owned()),
            Some("03:04:05.006007+02".to_owned()),
        ]]
    );
}

#[test]
fn should_bind_pg_catalog_qualified_temporal_casts_with_native_metadata() {
    // Arrange
    let sql = "SELECT $1::pg_catalog.timestamptz(3) AS timestamp_value, \
        $2::pg_catalog.timetz(3) AS time_value, \
        $3::pg_catalog.numeric(5,2) AS numeric_value";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "qualified_temporal_casts",
            sql,
            &[0, 0, 0],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("qualified_temporal_casts"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "qualified_temporal_portal",
                "qualified_temporal_casts",
                &[
                    "2001-02-16 20:38:40-05",
                    "20:38:40-05",
                    "12.345",
                ],
            ),
            wire::describe_portal_frame("qualified_temporal_portal"),
            wire::execute_frame("qualified_temporal_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-796-qualified-temporal-casts", steps);
    let parsed = &batches[1];
    let described = &batches[2];
    let executed = &batches[3];

    // Assert
    for (stage, frames) in [
        ("Parse", parsed),
        ("statement Describe", described),
        ("Bind/portal Describe/Execute", executed),
    ] {
        assert_eq!(
            wire::error_code(frames),
            None,
            "{stage} ErrorResponse: {:?}",
            frames
                .iter()
                .filter(|(tag, _)| *tag == b'E')
                .map(|(_, payload)| wire::parse_error_fields(payload))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("qualified cast ParameterDescription")
                .1
        ),
        vec![1184, 1266, 1700]
    );
    let statement_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("qualified cast statement RowDescription");
    let portal_description = executed
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("qualified cast portal RowDescription");
    let expected_columns = vec![
        ("timestamp_value", 1184, 8, 3, 0),
        ("time_value", 1266, 12, 3, 0),
        ("numeric_value", 1700, -1, 327_686, 0),
    ];
    for description in [statement_description, portal_description] {
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|column| {
                    (
                        column.name.as_str(),
                        column.type_oid,
                        column.type_size,
                        column.type_mod,
                        column.format_code,
                    )
                })
                .collect::<Vec<_>>(),
            expected_columns
        );
    }
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![
            Some("2001-02-17 01:38:40+00".to_owned()),
            Some("20:38:40-05".to_owned()),
            Some("12.35".to_owned()),
        ]]
    );
}
