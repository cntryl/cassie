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

#[test]
fn should_report_timestamp_excess_bind_parameter_ordinal_and_recover() {
    // Arrange
    use cassie::types::extended::temporal::calendar::END_TIMESTAMP;
    let good_integer = 1_i32.to_be_bytes();
    let extra_valid = [0_i64.to_be_bytes().as_slice(), &[0]].concat();
    let extra_invalid = [END_TIMESTAMP.to_be_bytes().as_slice(), &[0]].concat();
    let mut steps = Vec::new();
    for (index, explicit) in [false, true].into_iter().enumerate() {
        let first = format!("timestamp_extra_first_{index}");
        let second = format!("timestamp_extra_second_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &first,
            "SELECT $1, $2",
            &[1114, 23],
        )));
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &second,
            "SELECT $1, $2",
            &[23, 1184],
        )));
        if explicit {
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame("BEGIN")]));
        }
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                &first,
                &[1],
                &[Some(&extra_valid), Some(&good_integer)],
                &[],
            ),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
        if explicit {
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                "ROLLBACK",
            )]));
        }
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                &second,
                &[1],
                &[Some(&good_integer), Some(&extra_valid)],
                &[],
            ),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                &second,
                &[1],
                &[Some(&good_integer), Some(&extra_invalid)],
                &[],
            ),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", &first, &[1], &[None, Some(&good_integer)], &[]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 45",
        )]));
    }

    // Act
    let batches = run_script("timestamp-bind-extra-ordinal", steps);

    // Assert
    for (first, second, invalid, null, recovery) in [(2, 3, 4, 5, 6), (10, 12, 13, 14, 15)] {
        for (index, ordinal) in [(first, 1), (second, 2)] {
            assert_eq!(wire::error_code(&batches[index]).as_deref(), Some("22P03"));
            let fields = wire::parse_error_fields(
                &batches[index]
                    .iter()
                    .find(|frame| frame.0 == b'E')
                    .expect("error response")
                    .1,
            );
            assert!(fields.iter().any(|(field, message)| *field == 'M'
                && message
                    == &format!("incorrect binary data format in bind parameter {ordinal}")));
        }
        assert_eq!(
            wire::error_code(&batches[invalid]).as_deref(),
            Some("22008")
        );
        assert_eq!(
            wire::data_rows(&batches[null]),
            vec![vec![None, Some("1".to_owned())]]
        );
        assert_eq!(
            wire::data_rows(&batches[recovery]),
            vec![vec![Some("45".to_owned())]]
        );
    }
    assert_eq!(
        batches[10].last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
}

#[test]
fn should_validate_binary_date_and_time_bounds_and_recover() {
    // Arrange
    use cassie::types::extended::temporal::calendar::{
        DATE_END_JULIAN, POSTGRES_EPOCH_JDATE, USECS_PER_DAY,
    };
    let first = -POSTGRES_EPOCH_JDATE;
    let end = i32::try_from(DATE_END_JULIAN - i64::from(POSTGRES_EPOCH_JDATE)).expect("end");
    let cases: [(i32, Vec<(Vec<u8>, Option<&str>)>); 2] = [
        (
            1082,
            vec![
                (i32::MIN.to_be_bytes().to_vec(), None),
                (first.to_be_bytes().to_vec(), None),
                ((end - 1).to_be_bytes().to_vec(), None),
                (i32::MAX.to_be_bytes().to_vec(), None),
                ((first - 1).to_be_bytes().to_vec(), Some("22008")),
                (end.to_be_bytes().to_vec(), Some("22008")),
                (vec![0; 3], Some("08P01")),
                (vec![0; 5], Some("22P03")),
                (
                    [(first - 1).to_be_bytes().as_slice(), &[0]].concat(),
                    Some("22008"),
                ),
            ],
        ),
        (
            1083,
            vec![
                (0_i64.to_be_bytes().to_vec(), None),
                (USECS_PER_DAY.to_be_bytes().to_vec(), None),
                ((-1_i64).to_be_bytes().to_vec(), Some("22008")),
                ((USECS_PER_DAY + 1).to_be_bytes().to_vec(), Some("22008")),
                (vec![0; 7], Some("08P01")),
                (vec![0; 9], Some("22P03")),
                (
                    [(-1_i64).to_be_bytes().as_slice(), &[0]].concat(),
                    Some("22008"),
                ),
            ],
        ),
    ];
    let mut steps = Vec::new();
    let mut expected = Vec::new();
    for (index, (oid, values)) in cases.iter().enumerate() {
        let statement = format!("date_time_binary_bounds_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &statement,
            "SELECT $1",
            &[*oid],
        )));
        expected.push(None);
        for (payload, state) in values {
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats("", &statement, &[1], &[Some(payload)], &[]),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
            expected.push(*state);
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 1",
            )]));
            expected.push(None);
        }
    }

    // Act
    let batches = run_script("date-time-binary-receive-bounds", steps);

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
fn should_report_date_time_bind_extra_parameter_ordinal() {
    // Arrange
    let date = [0_i32.to_be_bytes().as_slice(), &[0]].concat();
    let time = [0_i64.to_be_bytes().as_slice(), &[0]].concat();
    let integer = 1_i32.to_be_bytes();
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "date_extra_ordinal",
            "SELECT $1, $2",
            &[1082, 23],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "date_extra_ordinal",
                &[1],
                &[Some(&date), Some(&integer)],
                &[],
            ),
            wire::sync_frame(),
        ]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "time_extra_ordinal",
            "SELECT $1, $2",
            &[23, 1083],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "time_extra_ordinal",
                &[1],
                &[Some(&integer), Some(&time)],
                &[],
            ),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("date-time-binary-extra-ordinal", steps);

    // Assert
    for (index, ordinal) in [(1, 1), (3, 2)] {
        assert_eq!(wire::error_code(&batches[index]).as_deref(), Some("22P03"));
        let fields = wire::parse_error_fields(
            &batches[index]
                .iter()
                .find(|frame| frame.0 == b'E')
                .expect("error")
                .1,
        );
        assert!(fields.iter().any(|(field, message)| *field == 'M'
            && message == &format!("incorrect binary data format in bind parameter {ordinal}")));
    }
}

#[test]
fn should_preserve_date_time_binary_array_element_error_order() {
    // Arrange
    let cases = [
        (
            1182,
            1082_i32,
            (-2_451_546_i32).to_be_bytes().to_vec(),
            4_usize,
        ),
        (1183, 1083_i32, (-1_i64).to_be_bytes().to_vec(), 8_usize),
    ];
    let mut steps = Vec::new();
    for (index, (array_oid, element_oid, invalid, width)) in cases.iter().enumerate() {
        let statement = format!("date_time_array_bounds_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &statement,
            "SELECT $1",
            &[*array_oid],
        )));
        for field in [invalid.clone(), [vec![0; *width], vec![0]].concat()] {
            let mut array = Vec::new();
            for part in [1_i32, 0, *element_oid, 2, 1] {
                array.extend_from_slice(&part.to_be_bytes());
            }
            array.extend_from_slice(&i32::try_from(*width).expect("width").to_be_bytes());
            array.extend(std::iter::repeat_n(0, *width));
            array.extend_from_slice(
                &i32::try_from(field.len())
                    .expect("field length")
                    .to_be_bytes(),
            );
            array.extend_from_slice(&field);
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats("", &statement, &[1], &[Some(&array)], &[]),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
        }
    }

    // Act
    let batches = run_script("date-time-array-binary-bounds", steps);

    // Assert
    for offset in [0, 3] {
        assert_eq!(
            wire::error_code(&batches[offset + 1]).as_deref(),
            Some("22008")
        );
        assert_eq!(
            wire::error_code(&batches[offset + 2]).as_deref(),
            Some("22P03")
        );
        let fields = wire::parse_error_fields(
            &batches[offset + 2]
                .iter()
                .find(|frame| frame.0 == b'E')
                .expect("error")
                .1,
        );
        assert!(fields.iter().any(|(field, message)| *field == 'M'
            && message == "improper binary format in array element 2"));
    }
}

#[test]
fn should_report_timetz_excess_bind_parameter_ordinal_and_recover() {
    // Arrange
    let integer = 1_i32.to_be_bytes();
    let frame = |time: i64, zone: i32| {
        [
            time.to_be_bytes().as_slice(),
            zone.to_be_bytes().as_slice(),
            &[0],
        ]
        .concat()
    };
    let valid_extra = frame(0, 0);
    let invalid_time_extra = frame(-1, 0);
    let invalid_zone_extra = frame(0, 57_600);
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "timetz_extra_first",
            "SELECT $1, $2",
            &[1266, 23],
        )),
        Step::ParseFlush(wire::parse_frame_with_types(
            "timetz_extra_second",
            "SELECT $1, $2",
            &[23, 1266],
        )),
        Step::ThroughReady(vec![wire::simple_query_frame("BEGIN")]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "timetz_extra_first",
                &[1],
                &[Some(&valid_extra), Some(&integer)],
                &[],
            ),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("ROLLBACK")]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "timetz_extra_second",
                &[1],
                &[Some(&integer), Some(&valid_extra)],
                &[],
            ),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "timetz_extra_second",
                &[1],
                &[Some(&integer), Some(&invalid_time_extra)],
                &[],
            ),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "timetz_extra_second",
                &[1],
                &[Some(&integer), Some(&invalid_zone_extra)],
                &[],
            ),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "timetz_extra_first",
                &[1],
                &[None, Some(&integer)],
                &[],
            ),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45")]),
    ];

    // Act
    let batches = run_script("timetz-bind-extra-ordinal", steps);

    // Assert
    for (index, ordinal) in [(3, 1), (5, 2)] {
        assert_eq!(wire::error_code(&batches[index]).as_deref(), Some("22P03"));
        let fields = wire::parse_error_fields(
            &batches[index]
                .iter()
                .find(|frame| frame.0 == b'E')
                .expect("error")
                .1,
        );
        assert!(fields.iter().any(|(field, message)| *field == 'M'
            && message == &format!("incorrect binary data format in bind parameter {ordinal}")));
    }
    assert_eq!(
        batches[3].last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(wire::error_code(&batches[6]).as_deref(), Some("22008"));
    assert_eq!(wire::error_code(&batches[7]).as_deref(), Some("22009"));
    assert_eq!(
        wire::data_rows(&batches[8]),
        vec![vec![None, Some("1".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[9]),
        vec![vec![Some("45".to_owned())]]
    );
}

#[test]
fn should_preserve_binary_interval_framing_and_bind_ordinal() {
    // Arrange
    let valid = [
        i64::MIN.to_be_bytes().as_slice(),
        i32::MAX.to_be_bytes().as_slice(),
        i32::MIN.to_be_bytes().as_slice(),
    ]
    .concat();
    let short = vec![0; 15];
    let extra = [valid.as_slice(), &[0]].concat();
    let integer = 1_i32.to_be_bytes();
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "interval_frame",
            "SELECT $1, $2",
            &[23, 1186],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "interval_frame",
                &[1],
                &[Some(&integer), Some(&valid)],
                &[],
            ),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "interval_frame",
                &[1],
                &[Some(&integer), Some(&short)],
                &[],
            ),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "interval_frame",
                &[1],
                &[Some(&integer), Some(&extra)],
                &[],
            ),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45")]),
    ];

    // Act
    let batches = run_script("interval-binary-framing", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("08P01"));
    assert_eq!(wire::error_code(&batches[3]).as_deref(), Some("22P03"));
    let fields = wire::parse_error_fields(
        &batches[3]
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("error")
            .1,
    );
    assert!(fields.iter().any(|(field, message)| *field == 'M'
        && message == "incorrect binary data format in bind parameter 2"));
    assert_eq!(
        wire::data_rows(&batches[4]),
        vec![vec![Some("45".to_owned())]]
    );
}

#[test]
fn should_preserve_binary_interval_array_framing_order() {
    // Arrange
    let mut steps = vec![Step::ParseFlush(wire::parse_frame_with_types(
        "interval_array_frame",
        "SELECT $1",
        &[1187],
    ))];
    for field in [vec![0; 15], vec![0; 17]] {
        let mut array = Vec::new();
        for number in [1_i32, 0, 1186, 2, 1, 16] {
            array.extend_from_slice(&number.to_be_bytes());
        }
        array.extend_from_slice(&[0; 16]);
        array.extend_from_slice(
            &i32::try_from(field.len())
                .expect("field size")
                .to_be_bytes(),
        );
        array.extend_from_slice(&field);
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "interval_array_frame", &[1], &[Some(&array)], &[]),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("interval-array-binary-framing", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("08P01"));
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22P03"));
    let fields = wire::parse_error_fields(
        &batches[2]
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("error")
            .1,
    );
    assert!(fields
        .iter()
        .any(|(field, message)| *field == 'M'
            && message == "improper binary format in array element 2"));
}
