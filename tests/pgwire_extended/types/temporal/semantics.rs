#[test]
fn should_preserve_temporal_range_endpoints_in_wire_results() {
    // Arrange
    let sql = "SELECT DATE '4713-01-01 BC' AS earliest_date, DATE '5874897-12-31' AS latest_date, TIMESTAMP '4713-01-01 BC' AS earliest_timestamp, TIMESTAMP '294276-12-31 23:59:59.999999' AS latest_timestamp, TIMESTAMPTZ '4713-01-01 00:00:00+00 BC' AS earliest_timestamptz, TIMESTAMPTZ '294276-12-31 23:59:59.999999+00' AS latest_timestamptz";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SET DateStyle = 'ISO, MDY'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        Step::ParseFlush(wire::parse_frame("temporal_range_endpoints", sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "temporal_range_endpoints_binary",
                "temporal_range_endpoints",
                &[],
                &[],
                &[1, 1, 1, 1, 1, 1],
            ),
            wire::describe_portal_frame("temporal_range_endpoints_binary"),
            wire::execute_frame("temporal_range_endpoints_binary"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-range-endpoints", steps);

    // Assert
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
    let text_description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("text temporal endpoint RowDescription");
    let text_columns = wire::parse_row_description(&text_description.1);
    assert_eq!(
        text_columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![
            (1082, 4, 0),
            (1082, 4, 0),
            (1114, 8, 0),
            (1114, 8, 0),
            (1184, 8, 0),
            (1184, 8, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("4713-01-01 BC".to_owned()),
            Some("5874897-12-31".to_owned()),
            Some("4713-01-01 00:00:00 BC".to_owned()),
            Some("294276-12-31 23:59:59.999999".to_owned()),
            Some("4713-01-01 00:00:00+00 BC".to_owned()),
            Some("294276-12-31 23:59:59.999999+00".to_owned()),
        ]]
    );

    let binary_description = batches[4]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary temporal endpoint RowDescription");
    let binary_columns = wire::parse_row_description(&binary_description.1);
    assert!(binary_columns.iter().all(|column| column.format_code == 1));
    assert_eq!(
        binary_columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1082, 1082, 1114, 1114, 1184, 1184]
    );
    assert_eq!(
        binary_row(&batches[4]),
        vec![
            vec![0xff, 0xda, 0x97, 0xcd],
            vec![0x7f, 0xda, 0x97, 0x0c],
            vec![0xfd, 0x0f, 0x7f, 0xbd, 0xaf, 0x17, 0xe0, 0x00],
            vec![0x7f, 0xff, 0xff, 0x5b, 0xb3, 0xb2, 0x9f, 0xff],
            vec![0xfd, 0x0f, 0x7f, 0xbd, 0xaf, 0x17, 0xe0, 0x00],
            vec![0x7f, 0xff, 0xff, 0x5b, 0xb3, 0xb2, 0x9f, 0xff],
        ]
    );
}

#[test]
fn should_cross_convert_temporal_infinity_result_formats() {
    // Arrange
    let date_positive = i32::MAX.to_be_bytes();
    let date_negative = i32::MIN.to_be_bytes();
    let timestamp_positive = i64::MAX.to_be_bytes();
    let timestamp_negative = i64::MIN.to_be_bytes();
    let cases = [
        (
            "date_positive_infinity_cross_format",
            1_082,
            "infinity",
            &date_positive[..],
            "infinity",
        ),
        (
            "date_negative_infinity_cross_format",
            1_082,
            "-infinity",
            &date_negative[..],
            "-infinity",
        ),
        (
            "timestamp_positive_infinity_cross_format",
            1_114,
            "infinity",
            &timestamp_positive[..],
            "infinity",
        ),
        (
            "timestamp_negative_infinity_cross_format",
            1_114,
            "-infinity",
            &timestamp_negative[..],
            "-infinity",
        ),
        (
            "timestamptz_positive_infinity_cross_format",
            1_184,
            "infinity",
            &timestamp_positive[..],
            "infinity",
        ),
        (
            "timestamptz_negative_infinity_cross_format",
            1_184,
            "-infinity",
            &timestamp_negative[..],
            "-infinity",
        ),
    ];
    let mut steps = Vec::new();
    for (statement, oid, text_input, binary_input, _) in &cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1 AS value",
            &[*oid],
        )));

        let binary_input_portal = format!("{statement}_binary_input_text_result");
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                &binary_input_portal,
                statement,
                &[1],
                &[Some(*binary_input)],
                &[0],
            ),
            wire::describe_portal_frame(&binary_input_portal),
            wire::execute_frame(&binary_input_portal),
            wire::sync_frame(),
        ]));

        let text_input_portal = format!("{statement}_text_input_binary_result");
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                &text_input_portal,
                statement,
                &[0],
                &[Some(text_input.as_bytes())],
                &[1],
            ),
            wire::describe_portal_frame(&text_input_portal),
            wire::execute_frame(&text_input_portal),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("postgresql18-temporal-infinity-cross-formats", steps);

    // Assert
    for (index, (statement, oid, _, binary, expected_text)) in cases.into_iter().enumerate() {
        assert_temporal_infinity_case(
            statement,
            oid,
            binary,
            expected_text,
            &batches[index * 3..index * 3 + 3],
        );
    }
}

fn assert_temporal_infinity_case(
    statement: &str,
    oid: i32,
    binary: &[u8],
    expected_text: &str,
    batches: &[Vec<(u8, Vec<u8>)>],
) {
    let parse_result = &batches[0];
    let binary_input_text_result = &batches[1];
    let text_input_binary_result = &batches[2];
    assert_eq!(wire::error_code(parse_result), None, "{statement} Parse");
    assert_eq!(
        parse_result.first().map(|(tag, _)| *tag),
        Some(b'1'),
        "{statement} ParseComplete"
    );
    assert_eq!(
        wire::error_code(binary_input_text_result),
        None,
        "{statement} binary input error fields: {:?}",
        binary_input_text_result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let text_description = binary_input_text_result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("text infinity RowDescription");
    let text_column = &wire::parse_row_description(&text_description.1)[0];
    assert_eq!((text_column.type_oid, text_column.format_code), (oid, 0));
    assert_eq!(
        wire::data_rows(binary_input_text_result),
        vec![vec![Some(expected_text.to_owned())]],
        "{statement} binary input to text result"
    );

    assert_eq!(
        wire::error_code(text_input_binary_result),
        None,
        "{statement} text input error fields: {:?}",
        text_input_binary_result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let binary_description = text_input_binary_result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary infinity RowDescription");
    let binary_column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(
        (binary_column.type_oid, binary_column.format_code),
        (oid, 1)
    );
    assert_eq!(
        binary_row(text_input_binary_result),
        vec![binary.to_vec()],
        "{statement} text input to binary result"
    );
}

#[test]
fn should_preserve_named_session_timezone_setting() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET TIME ZONE 'America/New_York'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SHOW TIME ZONE")]),
    ];

    // Act
    let batches = run_script("postgresql18-named-session-timezone", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "named timezone SET error fields: {:?}",
        batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("America/New_York".to_owned())]]
    );
}

#[test]
fn should_render_timestamptz_across_daylight_saving_in_the_session_zone() {
    // Arrange
    let sql = "SELECT TIMESTAMPTZ '2024-03-10 06:59:59+00' AS before_spring_forward, TIMESTAMPTZ '2024-03-10 07:00:00+00' AS after_spring_forward";
    let statement = "dst_session_zone_output";
    let portal = "dst_session_zone_binary_output";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET TIME ZONE 'America/New_York'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-timestamptz-dst-session-zone-output", steps);

    // Assert
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
    let text_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("DST text RowDescription");
    assert_eq!(
        wire::parse_row_description(&text_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1184, 1184]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![
            Some("2024-03-10 01:59:59-05".to_owned()),
            Some("2024-03-10 03:00:00-04".to_owned()),
        ]]
    );

    let binary_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("DST binary RowDescription");
    let columns = wire::parse_row_description(&binary_description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.format_code))
            .collect::<Vec<_>>(),
        vec![(1184, 1), (1184, 1)]
    );
    assert_eq!(
        binary_row(&batches[3]),
        vec![
            763_369_199_000_000_i64.to_be_bytes().to_vec(),
            763_369_200_000_000_i64.to_be_bytes().to_vec(),
        ]
    );
}

#[test]
fn should_render_all_interval_style_outputs() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET IntervalStyle = 'postgres'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT INTERVAL '1 year 2 months 3 days 4 hours 5 minutes 6.007 seconds'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET IntervalStyle = 'postgres_verbose'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT INTERVAL '1 year 2 months 3 days 4 hours 5 minutes 6.007 seconds'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET IntervalStyle = 'sql_standard'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT INTERVAL '1 year 2 months 3 days 4 hours 5 minutes 6.007 seconds'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET IntervalStyle = 'iso_8601'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT INTERVAL '1 year 2 months 3 days 4 hours 5 minutes 6.007 seconds'",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-interval-style-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(wire::error_code(&batches[3]), None);
    assert_eq!(wire::error_code(&batches[4]), None);
    assert_eq!(wire::error_code(&batches[5]), None);
    assert_eq!(wire::error_code(&batches[6]), None);
    assert_eq!(wire::error_code(&batches[7]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("1 year 2 mons 3 days 04:05:06.007".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some(
            "@ 1 year 2 mons 3 days 4 hours 5 mins 6.007 secs".to_owned()
        )]]
    );
    assert_eq!(
        wire::data_rows(&batches[5]),
        vec![vec![Some("+1-2 +3 +4:05:06.007".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[7]),
        vec![vec![Some("P1Y2M3DT4H5M6.007S".to_owned())]]
    );
}
