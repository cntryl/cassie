//! PostgreSQL 18 temporal function protocol behavior tracked by #815.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use tokio::io::AsyncWriteExt;

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
fn should_reject_out_of_range_make_time_hour() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT MAKE_TIME(24, 0, 0)")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 48")]),
    ];

    // Act
    let batches = run_script("postgresql18-make-time-hour-range", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22008"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("48".to_owned())]]
    );
}

struct FixtureDirectory(String);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_cancel_pg_sleep_for() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("postgresql18-pg-sleep-cancel");
    let _directory = FixtureDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let (cancelled, recovered) = runtime.block_on(async {
        let cassie = cassie::app::Cassie::new_with_data_dir(&path).expect("Cassie");
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
