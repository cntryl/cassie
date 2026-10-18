#[test]
fn should_return_postgresql_binary_temporal_encodings() {
    // Arrange
    let sql = "SELECT DATE '2000-01-02' AS d, TIME '03:04:05.006007' AS t, TIMESTAMP '2000-01-02 03:04:05.006007' AS ts, TIMESTAMPTZ '2000-01-02 03:04:05.006007+00' AS tz, INTERVAL '1 mon 2 days 03:04:05.006007' AS i, TIMETZ '03:04:05.006007+02' AS tztime";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("temporal_binary_output", sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "temporal_binary_portal",
                "temporal_binary_output",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("temporal_binary_portal"),
            wire::execute_frame("temporal_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-binary-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary temporal RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert!(columns.iter().all(|column| column.format_code == 1));
    assert_eq!(
        binary_row(&batches[1]),
        vec![
            1_i32.to_be_bytes().to_vec(),
            11_045_006_007_i64.to_be_bytes().to_vec(),
            97_445_006_007_i64.to_be_bytes().to_vec(),
            97_445_006_007_i64.to_be_bytes().to_vec(),
            [
                11_045_006_007_i64.to_be_bytes().as_slice(),
                2_i32.to_be_bytes().as_slice(),
                1_i32.to_be_bytes().as_slice(),
            ]
            .concat(),
            [
                11_045_006_007_i64.to_be_bytes().as_slice(),
                (-7_200_i32).to_be_bytes().as_slice(),
            ]
            .concat(),
        ]
    );
}

#[test]
fn should_decode_temporal_infinity_binary_parameters() {
    // Arrange
    let date_positive = i32::MAX.to_be_bytes();
    let date_negative = i32::MIN.to_be_bytes();
    let timestamp_positive = i64::MAX.to_be_bytes();
    let timestamp_negative = i64::MIN.to_be_bytes();
    let cases = [
        (
            "date_positive_infinity_binary",
            1_082,
            &date_positive[..],
            "infinity",
        ),
        (
            "date_negative_infinity_binary",
            1_082,
            &date_negative[..],
            "-infinity",
        ),
        (
            "timestamp_positive_infinity_binary",
            1_114,
            &timestamp_positive[..],
            "infinity",
        ),
        (
            "timestamp_negative_infinity_binary",
            1_114,
            &timestamp_negative[..],
            "-infinity",
        ),
        (
            "timestamptz_positive_infinity_binary",
            1_184,
            &timestamp_positive[..],
            "infinity",
        ),
        (
            "timestamptz_negative_infinity_binary",
            1_184,
            &timestamp_negative[..],
            "-infinity",
        ),
    ];
    let mut steps = Vec::new();
    for (statement, oid, payload, _) in &cases {
        let portal = format!("{statement}_portal");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1 AS value",
            &[*oid],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(&portal, statement, &[1], &[Some(*payload)], &[1]),
            wire::describe_portal_frame(&portal),
            wire::execute_frame(&portal),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("issue-814-temporal-infinity-binary-input", steps);

    // Assert
    for (index, (statement, oid, expected, _)) in cases.into_iter().enumerate() {
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
            .expect("binary temporal infinity RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!((column.type_oid, column.format_code), (oid, 1));
        assert_eq!(binary_row(result), vec![expected.to_vec()]);
    }
}

#[test]
fn should_accept_binary_temporal_parameters_with_explicit_postgresql_oids() {
    // Arrange
    let sql = "SELECT $1 AS d, $2 AS t, $3 AS ts, $4 AS tz, $5 AS i, $6 AS tztime";
    let interval = [
        11_045_006_007_i64.to_be_bytes().as_slice(),
        2_i32.to_be_bytes().as_slice(),
        1_i32.to_be_bytes().as_slice(),
    ]
    .concat();
    let timetz = [
        11_045_006_007_i64.to_be_bytes().as_slice(),
        (-7_200_i32).to_be_bytes().as_slice(),
    ]
    .concat();
    let date = 1_i32.to_be_bytes().to_vec();
    let time = 11_045_006_007_i64.to_be_bytes().to_vec();
    let timestamp = 97_445_006_007_i64.to_be_bytes().to_vec();
    let parameters = [
        Some(date.as_slice()),
        Some(time.as_slice()),
        Some(timestamp.as_slice()),
        Some(timestamp.as_slice()),
        Some(interval.as_slice()),
        Some(timetz.as_slice()),
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "temporal_binary_parameters",
            sql,
            &[1082, 1083, 1114, 1184, 1186, 1266],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "temporal_binary_portal",
                "temporal_binary_parameters",
                &[1],
                &parameters,
                &[1],
            ),
            wire::describe_portal_frame("temporal_binary_portal"),
            wire::execute_frame("temporal_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-binary-parameters", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        binary_row(&batches[1]),
        vec![
            1_i32.to_be_bytes().to_vec(),
            11_045_006_007_i64.to_be_bytes().to_vec(),
            97_445_006_007_i64.to_be_bytes().to_vec(),
            97_445_006_007_i64.to_be_bytes().to_vec(),
            interval,
            timetz,
        ]
    );
}
