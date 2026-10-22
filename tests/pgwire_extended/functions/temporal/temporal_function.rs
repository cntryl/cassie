//! PostgreSQL 18 temporal function protocol behavior tracked by #815.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use tokio::io::AsyncWriteExt;

#[path = "clock_observation.rs"]
mod clock_observation_contracts;
#[path = "offset_formatting.rs"]
mod offset_formatting;

#[test]
fn should_describe_temporal_function_results_with_postgresql_types() {
    // Arrange
    let sql = "SELECT NOW() AS now_value, CURRENT_DATE AS current_date, CURRENT_TIME(3) AS current_time, CURRENT_TIMESTAMP(3) AS current_timestamp, LOCALTIME(3) AS local_time, LOCALTIMESTAMP(3) AS local_timestamp, DATE_TRUNC('day', TIMESTAMP '2001-02-16 20:38:40') AS truncated_timestamp, DATE_TRUNC('day', TIMESTAMPTZ '2001-02-16 20:38:40+00', 'UTC') AS truncated_timestamptz, DATE_PART('epoch', TIMESTAMPTZ '2000-01-01 00:00:00+00') AS date_part, EXTRACT(YEAR FROM DATE '2001-02-16') AS extracted, TO_CHAR(DATE '2001-02-16', 'YYYY') AS formatted, TO_DATE('2001-02-16', 'YYYY-MM-DD') AS parsed_date, TO_TIMESTAMP(0) AS parsed_timestamp, AGE(TIMESTAMP '2004-06-01', TIMESTAMP '2004-04-30') AS age, DATE_BIN(INTERVAL '1 day', TIMESTAMP '2001-02-16 20:38:40', TIMESTAMP '2001-01-01') AS binned, ISFINITE(DATE '2001-02-16') AS finite, TIMEOFDAY() AS wall_clock";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-function-types", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("temporal function RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![
            1184, 1082, 1266, 1184, 1083, 1114, 1114, 1184, 701, 1700, 25, 1082, 1184, 1186, 1114,
            16, 25,
        ]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_mod)
            .collect::<Vec<_>>(),
        vec![-1, -1, 3, 3, 3, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1]
    );
    assert_eq!(wire::data_rows(&batches[1]).len(), 1);
    assert!(wire::data_rows(&batches[1])[0].iter().all(Option::is_some));
}

#[test]
fn should_describe_timezone_overloads_with_postgresql_result_types() {
    // Arrange
    let sql = "SELECT timezone('America/Denver', TIMESTAMP '2001-02-16 20:38:40') AS zoned_timestamp, timezone('America/Denver', TIMESTAMPTZ '2001-02-17 03:38:40+00') AS local_timestamp, timezone('UTC', TIME '05:34:17') AS zoned_time, timezone('UTC', TIMETZ '05:34:17-05') AS local_time";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
    ];

    // Act
    let batches = run_script("postgresql18-timezone-overload-result-types", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("timezone overload RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(1184, 8, -1), (1114, 8, -1), (1266, 12, -1), (1266, 12, -1),]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![
            Some("2001-02-17 03:38:40+00".to_owned()),
            Some("2001-02-16 20:38:40".to_owned()),
            Some("05:34:17+00".to_owned()),
            Some("10:34:17+00".to_owned()),
        ]]
    );
}

#[test]
fn should_return_null_for_temporal_function_results() {
    // Arrange
    let sql = "SELECT DATE_TRUNC('day', NULL::TIMESTAMP) AS truncated, TO_CHAR(NULL::DATE, 'YYYY') AS formatted, TO_DATE(NULL::TEXT, 'YYYY-MM-DD') AS parsed, AGE(NULL::TIMESTAMP, TIMESTAMP '2001-01-01') AS age";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-temporal-function-nulls", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("NULL temporal function RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1114, 25, 1082, 1186]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![None, None, None, None]]
    );
}

#[test]
fn should_bind_temporal_function_parameters_with_postgresql_types() {
    // Arrange
    let sql = "SELECT DATE_TRUNC('day', $1) AS truncated, AGE($2, $3) AS age, TO_CHAR($1, 'YYYY-MM-DD') AS formatted";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "temporal_function_parameters",
            sql,
            &[1114, 1114, 1114],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("temporal_function_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "temporal_function_portal",
                "temporal_function_parameters",
                &[
                    "2001-02-16 20:38:40",
                    "2004-06-01 00:00:00",
                    "2004-04-30 00:00:00",
                ],
            ),
            wire::describe_portal_frame("temporal_function_portal"),
            wire::execute_frame("temporal_function_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-function-parameters", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let parameters = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("temporal function ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![1114, 1114, 1114]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("2001-02-16 00:00:00".to_owned()),
            Some("1 mon 1 day".to_owned()),
            Some("2001-02-16".to_owned()),
        ]]
    );
}

#[test]
fn should_refresh_statement_timestamp_when_reusing_prepared_statement() {
    // Arrange
    let statement = "statement_clock_reuse";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ParseFlush(wire::parse_frame(
            statement,
            "SELECT STATEMENT_TIMESTAMP() AS value",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("first_clock_portal", statement, &[], &[], &[]),
            wire::describe_portal_frame("first_clock_portal"),
            wire::execute_frame("first_clock_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT PG_SLEEP_FOR(INTERVAL '50 milliseconds')",
        )]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("second_clock_portal", statement, &[], &[], &[]),
            wire::describe_portal_frame("second_clock_portal"),
            wire::execute_frame("second_clock_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-prepared-statement-clock-volatility", steps);

    // Assert
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
    let first_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("first prepared statement portal RowDescription");
    let second_description = batches[5]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("second prepared statement portal RowDescription");
    for description in [first_description, second_description] {
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!((column.type_oid, column.type_mod), (1184, -1));
    }
    let first_value = wire::data_rows(&batches[3])[0][0]
        .clone()
        .expect("first statement timestamp");
    let second_value = wire::data_rows(&batches[5])[0][0]
        .clone()
        .expect("second statement timestamp");
    assert_ne!(
        first_value, second_value,
        "reusing a prepared statement evaluates statement_timestamp for each execution"
    );
}

#[test]
fn should_reformat_prepared_timestamptz_after_session_timezone_changes() {
    // Arrange
    let statement = "prepared_timestamptz_session_zone";
    let value = "2024-03-10 12:00:00+00";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1 AS value",
            &[1184],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("prepared_utc_portal_before", statement, &[value]),
            wire::describe_portal_frame("prepared_utc_portal_before"),
            wire::execute_frame("prepared_utc_portal_before"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET TIME ZONE 'America/New_York'",
        )]),
        Step::ThroughReady(vec![
            wire::bind_frame("prepared_new_york_portal", statement, &[value]),
            wire::describe_portal_frame("prepared_new_york_portal"),
            wire::execute_frame("prepared_new_york_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![
            wire::bind_frame("prepared_utc_portal_after", statement, &[value]),
            wire::describe_portal_frame("prepared_utc_portal_after"),
            wire::execute_frame("prepared_utc_portal_after"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-prepared-timestamptz-timezone", steps);

    // Assert
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
    let parameter_description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("prepared timestamptz ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![1184]
    );
    let statement_description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared timestamptz statement RowDescription");
    let statement_column = &wire::parse_row_description(&statement_description.1)[0];
    assert_eq!(
        (
            statement_column.type_oid,
            statement_column.type_size,
            statement_column.type_mod,
        ),
        (1184, 8, -1)
    );

    for batch in [&batches[3], &batches[5], &batches[7]] {
        let description = batch
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("prepared timestamptz portal RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!((column.type_oid, column.format_code), (1184, 0));
    }
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("2024-03-10 12:00:00+00".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[5]),
        vec![vec![Some("2024-03-10 08:00:00-04".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[7]),
        vec![vec![Some("2024-03-10 12:00:00+00".to_owned())]]
    );
}

#[test]
fn should_report_invalid_calendar_constructor_input() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT MAKE_DATE(2001, 2, 30)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("postgresql18-make-date-invalid-recovery", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22008"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_report_invalid_formatted_date_input() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TO_DATE('not-a-date', 'YYYY-MM-DD')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 43")]),
    ];

    // Act
    let batches = run_script("postgresql18-to-date-invalid-recovery", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22007"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("43".to_owned())]]
    );
}

#[test]
fn should_reject_calendar_units_in_date_bin_stride() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT DATE_BIN(INTERVAL '1 month', TIMESTAMP '2020-01-01', TIMESTAMP '2000-01-01')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 44")]),
    ];

    // Act
    let batches = run_script("postgresql18-date-bin-month-recovery", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("44".to_owned())]]
    );
}

#[test]
fn should_reject_zero_date_bin_stride() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT DATE_BIN(INTERVAL '0 seconds', TIMESTAMP '2020-01-01', TIMESTAMP '2000-01-01')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 46")]),
    ];

    // Act
    let batches = run_script("postgresql18-date-bin-zero-stride", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22008"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("46".to_owned())]]
    );
}

#[test]
fn should_return_postgresql_date_bin_timestamptz_output() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT DATE_BIN('24 hours', '2024-03-10 16:30:00+00', '2024-03-09 17:00:00+00')",
        )]),
    ];

    // Act
    let batches = run_script("issue-815-date-bin-timestamptz-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        wire::error_code(&batches[1]),
        None,
        "date_bin execution error: {:?}",
        batches[1]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("date_bin timestamptz RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.format_code),
        (1184, 8, 0)
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("2024-03-09 17:00:00+00".to_owned())]]
    );
}

#[test]
fn should_reject_invalid_formatted_timestamp_input() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TO_TIMESTAMP('not-a-timestamp', 'YYYY-MM-DD HH24:MI:SS')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 47")]),
    ];

    // Act
    let batches = run_script("postgresql18-to-timestamp-invalid", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22007"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("47".to_owned())]]
    );
}

#[test]
fn should_accept_make_time_hour_24() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT MAKE_TIME(24, 0, 0)",
    )])];

    // Act
    let batches = run_script("postgresql18-make-time-hour-24", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("MAKE_TIME RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1083
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("24:00:00".to_owned())]]
    );
}

struct FixtureDirectory(String);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_bind_date_add_parameters_with_dst_zone() {
    // Arrange
    let sql = "SELECT DATE_ADD($1, $2, $3) AS calendar_day";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET TIME ZONE 'America/New_York'",
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "date_add_dst_parameters",
            sql,
            &[1184, 1186, 25],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("date_add_dst_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "date_add_dst_portal",
                "date_add_dst_parameters",
                &["2024-03-09 12:00:00-05", "1 day", "America/New_York"],
            ),
            wire::describe_portal_frame("date_add_dst_portal"),
            wire::execute_frame("date_add_dst_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-date-add-dst-bind", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        wire::error_code(&batches[1]),
        None,
        "Parse error fields: {:?}",
        batches[1]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(&batches[2]),
        None,
        "Describe error fields: {:?}",
        batches[2]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::parse_parameter_description(
            &batches[2]
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("date_add ParameterDescription")
                .1,
        ),
        vec![1184, 1186, 25]
    );
    assert_eq!(wire::error_code(&batches[3]), None);
    let description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("DATE_ADD RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1184
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("2024-03-10 12:00:00-04".to_owned())]]
    );
}

#[test]
fn should_bind_date_subtract_parameters_with_dst_zone() {
    // Arrange
    let sql = "SELECT DATE_SUBTRACT($1, $2, $3) AS calendar_day";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET TIME ZONE 'America/New_York'",
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "date_subtract_dst_parameters",
            sql,
            &[1184, 1186, 25],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("date_subtract_dst_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "date_subtract_dst_portal",
                "date_subtract_dst_parameters",
                &["2024-03-11 12:00:00-04", "1 day", "America/New_York"],
            ),
            wire::describe_portal_frame("date_subtract_dst_portal"),
            wire::execute_frame("date_subtract_dst_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-date-subtract-dst-bind", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        wire::error_code(&batches[1]),
        None,
        "Parse error fields: {:?}",
        batches[1]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(&batches[2]),
        None,
        "Describe error fields: {:?}",
        batches[2]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::parse_parameter_description(
            &batches[2]
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("date_subtract ParameterDescription")
                .1,
        ),
        vec![1184, 1186, 25]
    );
    assert_eq!(wire::error_code(&batches[3]), None);
    let description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("DATE_SUBTRACT RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1184
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("2024-03-10 12:00:00-04".to_owned())]]
    );
}

#[test]
fn should_cancel_pg_sleep_for() {
    // Arrange
    let path = wire::data_dir("postgresql18-pg-sleep-cancel");
    let _directory = FixtureDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let (cancelled, recovered) = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
        cassie.startup().expect("start cancellation fixture");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT PG_SLEEP_FOR(INTERVAL '60 seconds')",
            )],
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let mut cancel_socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, std::time::Duration::from_secs(5))
                .await;
        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 45")]).await;
        let recovered =
            wire::read_frames_until_ready_within(&mut reader, std::time::Duration::from_secs(5))
                .await;
        drop(reader);
        drop(writer);
        server.stop().await;
        (cancelled, recovered)
    });

    // Assert
    assert_eq!(wire::error_code(&cancelled).as_deref(), Some("57014"));
    assert_eq!(wire::error_code(&recovered), None);
    assert_eq!(
        wire::data_rows(&recovered),
        vec![vec![Some("45".to_owned())]]
    );
}

#[test]
fn should_cancel_pg_sleep_until() {
    // Arrange
    let path = wire::data_dir("postgresql18-pg-sleep-until-cancel");
    let _directory = FixtureDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let (cancelled, recovered) = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
        cassie.startup().expect("start cancellation fixture");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT PG_SLEEP_UNTIL(CLOCK_TIMESTAMP() + INTERVAL '60 seconds')",
            )],
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let mut cancel_socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, std::time::Duration::from_secs(5))
                .await;
        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 45")]).await;
        let recovered =
            wire::read_frames_until_ready_within(&mut reader, std::time::Duration::from_secs(5))
                .await;
        drop(reader);
        drop(writer);
        server.stop().await;
        (cancelled, recovered)
    });

    // Assert
    assert_eq!(wire::error_code(&cancelled).as_deref(), Some("57014"));
    assert_eq!(wire::error_code(&recovered), None);
    assert_eq!(
        wire::data_rows(&recovered),
        vec![vec![Some("45".to_owned())]]
    );
}
