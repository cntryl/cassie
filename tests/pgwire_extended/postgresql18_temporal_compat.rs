//! PostgreSQL 18 temporal descriptors, codecs, settings, and errors tracked by #814.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn binary_row(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<u8>> {
    let payload = &frames
        .iter()
        .find(|(tag, _)| *tag == b'D')
        .expect("DataRow")
        .1;
    let mut cursor = 0_usize;
    let field_count =
        i16::from_be_bytes(payload[cursor..cursor + 2].try_into().expect("field count"));
    cursor += 2;
    let mut fields = Vec::new();
    for _ in 0..field_count {
        let length = i32::from_be_bytes(
            payload[cursor..cursor + 4]
                .try_into()
                .expect("field length"),
        );
        cursor += 4;
        assert!(length >= 0, "temporal output should not be NULL");
        let end = cursor + usize::try_from(length).expect("nonnegative field length");
        fields.push(payload[cursor..end].to_vec());
        cursor = end;
    }
    assert_eq!(cursor, payload.len(), "DataRow has no trailing bytes");
    fields
}

#[test]
fn should_describe_temporal_values_with_postgresql_identities() {
    // Arrange
    let sql = "SELECT DATE '2000-01-02' AS d, TIME '03:04:05.006007' AS t, TIMESTAMP '2000-01-02 03:04:05.006007' AS ts, TIMESTAMPTZ '2000-01-02 03:04:05.006007+00' AS tz, INTERVAL '1 mon 2 days 03:04:05.006007' AS i, TIMETZ '03:04:05.006007+02' AS tztime";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-identities", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("temporal RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1082, 1083, 1114, 1184, 1186, 1266]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_size)
            .collect::<Vec<_>>(),
        vec![4, 8, 8, 8, 16, 12]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
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
fn should_apply_temporal_typmods_in_protocol_metadata() {
    // Arrange
    let sql = "SELECT TIME(3) '03:04:05.0067' AS t, TIMESTAMP(2) '2000-01-02 03:04:05.006' AS ts, TIMESTAMPTZ(4) '2000-01-02 03:04:05.00678+00' AS tz, INTERVAL '5.0067 seconds' SECOND(3) AS i";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-temporal-typmods", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("temporal typmod RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1083, 1114, 1184, 1186]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_mod)
            .collect::<Vec<_>>(),
        vec![3, 2, 4, 268_435_459]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("03:04:05.007".to_owned()),
            Some("2000-01-02 03:04:05.01".to_owned()),
            Some("2000-01-02 03:04:05.0068+00".to_owned()),
            Some("00:00:05.007".to_owned()),
        ]]
    );
}

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
fn should_format_temporal_text_from_session_settings() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SET DateStyle = 'ISO, MDY'")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT DATE '1997-12-17'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET TIME ZONE 'America/New_York'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TIMESTAMPTZ '2000-01-02 03:04:05+00'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SET DateStyle = 'SQL, DMY'")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT DATE '1997-12-17'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET DateStyle = 'Postgres, DMY'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TIMESTAMPTZ '1997-12-17 00:00:00+00'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET DateStyle = 'German, DMY'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT DATE '1997-12-17'")]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-session-format", steps);

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("1997-12-17".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[4]),
        vec![vec![Some("2000-01-01 22:04:05-05".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[7]),
        vec![vec![Some("17/12/1997".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[9]),
        vec![vec![Some("Wed 17 Dec 00:00:00 1997 UTC".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[11]),
        vec![vec![Some("17.12.1997".to_owned())]]
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

#[test]
fn should_describe_temporal_nulls_with_declared_result_types() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT NULL::DATE, NULL::TIME, NULL::TIMESTAMP, NULL::TIMESTAMPTZ, NULL::INTERVAL, NULL::TIMETZ",
    )])];

    // Act
    let batches = run_script("postgresql18-temporal-null-descriptors", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("NULL temporal RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1082, 1083, 1114, 1184, 1186, 1266]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![None, None, None, None, None, None]]
    );
}

#[test]
fn should_return_datetime_error_sqlstate_for_invalid_timestamp_fields() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT TIMESTAMP '2024-02-30 12:00:00'",
    )])];

    // Act
    let batches = run_script("postgresql18-temporal-invalid-field", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22008"));
}

#[test]
fn should_recover_after_temporal_input_error_on_the_same_connection() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TIMESTAMP '2024-02-30 12:00:00'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TIMESTAMP '2024-02-29 12:00:00'",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-error-recovery", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22008"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("2024-02-29 12:00:00".to_owned())]]
    );
}
