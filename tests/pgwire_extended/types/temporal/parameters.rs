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
