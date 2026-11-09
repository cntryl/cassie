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

#[test]
fn should_validate_timetz_binary_bounds_and_recover_on_same_connection() {
    // Arrange
    let frame = |time: i64, zone: i32| {
        [time.to_be_bytes().as_slice(), zone.to_be_bytes().as_slice()].concat()
    };
    let mut overlong_valid = frame(0, 0);
    overlong_valid.push(0);
    let mut overlong_bad_time = frame(-1, 0);
    overlong_bad_time.push(0);
    let cases = [
        (frame(0, 0), None),
        (frame(86_400_000_000, -57_599), None),
        (frame(0, 57_599), None),
        (frame(-1, 0), Some("22008")),
        (frame(86_400_000_001, 0), Some("22008")),
        (frame(0, -57_600), Some("22009")),
        (frame(0, 57_600), Some("22009")),
        (frame(0, i32::MIN), Some("22009")),
        (frame(0, i32::MAX), Some("22009")),
        (frame(-1, i32::MIN), Some("22008")),
        (vec![0; 11], Some("08P01")),
        (overlong_valid, Some("22P03")),
        (overlong_bad_time, Some("22008")),
    ];
    let mut steps = vec![Step::ParseFlush(wire::parse_frame_with_types(
        "timetz_bounds",
        "SELECT $1",
        &[1266],
    ))];
    for (payload, _) in &cases {
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "timetz_bounds",
                &[1],
                &[Some(payload.as_slice())],
                &[0],
            ),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 1",
        )]));
    }
    // Act
    let batches = run_script("timetz-binary-receive-boundaries", steps);
    // Assert
    for (index, (_, expected)) in cases.iter().enumerate() {
        assert_eq!(
            wire::error_code(&batches[1 + index * 2]).as_deref(),
            *expected,
            "case {index}"
        );
        assert_eq!(
            wire::error_code(&batches[2 + index * 2]),
            None,
            "recovery {index}"
        );
        assert_eq!(
            wire::data_rows(&batches[2 + index * 2]),
            vec![vec![Some("1".to_owned())]]
        );
    }
}

#[test]
fn should_validate_binary_timestamp_and_timestamptz_bounds_and_recover() {
    // Arrange
    use cassie::types::extended::temporal::calendar::{END_TIMESTAMP, MIN_TIMESTAMP};
    let mut steps = Vec::new();
    let mut expected = Vec::new();
    for (index, oid) in [1114, 1184].into_iter().enumerate() {
        let statement = format!("timestamp_bounds_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &statement,
            "SELECT $1",
            &[oid],
        )));
        expected.push(None);
        let cases = [
            (i64::MIN.to_be_bytes().to_vec(), None),
            (MIN_TIMESTAMP.to_be_bytes().to_vec(), None),
            ((END_TIMESTAMP - 1).to_be_bytes().to_vec(), None),
            (i64::MAX.to_be_bytes().to_vec(), None),
            ((MIN_TIMESTAMP - 1).to_be_bytes().to_vec(), Some("22008")),
            (END_TIMESTAMP.to_be_bytes().to_vec(), Some("22008")),
            (vec![0; 7], Some("08P01")),
            (
                [0_i64.to_be_bytes().as_slice(), &[0]].concat(),
                Some("22P03"),
            ),
            (
                [END_TIMESTAMP.to_be_bytes().as_slice(), &[0]].concat(),
                Some("22008"),
            ),
        ];
        for (payload, state) in cases {
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats("", &statement, &[1], &[Some(&payload)], &[]),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
            expected.push(state);
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 1",
            )]));
            expected.push(None);
        }
    }

    // Act
    let batches = run_script("timestamp-binary-receive-bounds", steps);

    // Assert
    for (index, state) in expected.iter().enumerate() {
        assert_eq!(
            wire::error_code(&batches[index]).as_deref(),
            *state,
            "case {index}"
        );
    }
}

#[test]
fn should_preserve_binary_timestamp_array_element_error_order() {
    // Arrange
    use cassie::types::extended::temporal::calendar::END_TIMESTAMP;
    let mut steps = Vec::new();
    let mut expected = Vec::new();
    for (index, (array_oid, element_oid)) in
        [(1115, 1114_i32), (1185, 1184_i32)].into_iter().enumerate()
    {
        let statement = format!("timestamp_array_bounds_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &statement,
            "SELECT $1",
            &[array_oid],
        )));
        expected.push(None);
        for (value, extra, state) in [
            (END_TIMESTAMP, false, "22008"),
            (END_TIMESTAMP, true, "22008"),
            (0, true, "22P03"),
        ] {
            let mut array = Vec::new();
            for part in [1_i32, 0, element_oid, 2, 1] {
                array.extend_from_slice(&part.to_be_bytes());
            }
            array.extend_from_slice(&8_i32.to_be_bytes());
            array.extend_from_slice(&0_i64.to_be_bytes());
            array.extend_from_slice(&(8_i32 + i32::from(extra)).to_be_bytes());
            array.extend_from_slice(&value.to_be_bytes());
            if extra {
                array.push(0);
            }
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats("", &statement, &[1], &[Some(&array)], &[]),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
            expected.push(Some(state));
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 1",
            )]));
            expected.push(None);
        }
    }

    // Act
    let batches = run_script("timestamp-array-binary-bounds", steps);

    // Assert
    for (index, state) in expected.iter().enumerate() {
        assert_eq!(
            wire::error_code(&batches[index]).as_deref(),
            *state,
            "case {index}"
        );
    }
}
