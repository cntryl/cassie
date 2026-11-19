use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie_test_support::support_pgwire as wire;

const PARAMETER_OIDS: [i32; 7] = [25, 25, 1184, 1184, 1184, 1184, 25];
const RESULT_OIDS: [i32; 6] = [25, 1082, 1114, 20, 20, 20];

const DAY_TEMPLATE: &str = "WITH \
    device_a AS MATERIALIZED (SELECT device,event_at,amount FROM {table} \
        WHERE device=$1 AND event_at >= $3 AND event_at < $4), \
    device_b AS MATERIALIZED (SELECT device,event_at,amount FROM {table} \
        WHERE device=$2 AND event_at >= $3 AND event_at < $4), \
    bucket_totals AS ( \
        SELECT device,date_trunc('day',event_at,$7::text) AS boundary, \
               count(*) AS observations,sum(amount) AS total \
        FROM (SELECT * FROM device_a UNION ALL SELECT * FROM device_b) input \
        GROUP BY device,date_trunc('day',event_at,$7::text)), \
    grid AS ( \
        SELECT $1::text AS device,boundary FROM generate_series( \
            $5::timestamptz,$6::timestamptz,interval '1 day',$7::text) s(boundary) \
        WHERE boundary < $6 \
        UNION ALL \
        SELECT $2::text AS device,boundary FROM generate_series( \
            $5::timestamptz,$6::timestamptz,interval '1 day',$7::text) s(boundary) \
        WHERE boundary < $6), \
    joined AS ( \
        SELECT grid.device,grid.boundary,bucket_totals.observations,bucket_totals.total, \
               sum(CASE WHEN bucket_totals.observations IS NULL \
                        THEN 0::bigint ELSE 1::bigint END) \
                   OVER (PARTITION BY grid.device ORDER BY grid.boundary) AS run_id \
        FROM grid LEFT JOIN bucket_totals \
          ON bucket_totals.device=grid.device AND bucket_totals.boundary=grid.boundary), \
    carried AS ( \
        SELECT device,(boundary AT TIME ZONE $7::text)::date AS local_label, \
               boundary AT TIME ZONE 'UTC' AS boundary_utc, \
               coalesce(observations,0)::bigint AS observation_count,total, \
               first_value(total) OVER (PARTITION BY device,run_id ORDER BY boundary \
                   ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS carried_total \
        FROM joined) \
SELECT device,local_label,boundary_utc,observation_count,total,carried_total \
FROM carried ORDER BY device,boundary_utc";

const MONTH_TEMPLATE: &str = "WITH \
    device_a AS MATERIALIZED (SELECT device,event_at,amount FROM {table} \
        WHERE device=$1 AND event_at >= $3 AND event_at < $4), \
    device_b AS MATERIALIZED (SELECT device,event_at,amount FROM {table} \
        WHERE device=$2 AND event_at >= $3 AND event_at < $4), \
    bucket_totals AS ( \
        SELECT device,date_trunc('month',event_at,$7::text) AS boundary, \
               count(*) AS observations,sum(amount) AS total \
        FROM (SELECT * FROM device_a UNION ALL SELECT * FROM device_b) input \
        GROUP BY device,date_trunc('month',event_at,$7::text)), \
    grid AS ( \
        SELECT $1::text AS device,boundary FROM generate_series( \
            $5::timestamptz,$6::timestamptz,interval '1 month',$7::text) s(boundary) \
        WHERE boundary < $6 \
        UNION ALL \
        SELECT $2::text AS device,boundary FROM generate_series( \
            $5::timestamptz,$6::timestamptz,interval '1 month',$7::text) s(boundary) \
        WHERE boundary < $6), \
    joined AS ( \
        SELECT grid.device,grid.boundary,bucket_totals.observations,bucket_totals.total, \
               sum(CASE WHEN bucket_totals.observations IS NULL \
                        THEN 0::bigint ELSE 1::bigint END) \
                   OVER (PARTITION BY grid.device ORDER BY grid.boundary) AS run_id \
        FROM grid LEFT JOIN bucket_totals \
          ON bucket_totals.device=grid.device AND bucket_totals.boundary=grid.boundary), \
    carried AS ( \
        SELECT device,(boundary AT TIME ZONE $7::text)::date AS local_label, \
               boundary AT TIME ZONE 'UTC' AS boundary_utc, \
               coalesce(observations,0)::bigint AS observation_count,total, \
               first_value(total) OVER (PARTITION BY device,run_id ORDER BY boundary \
                   ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS carried_total \
        FROM joined) \
SELECT device,local_label,boundary_utc,observation_count,total,carried_total \
FROM carried ORDER BY device,boundary_utc";

type Frames = Vec<(u8, Vec<u8>)>;

struct PreparedClient {
    reader: tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf,
    statements: std::collections::HashMap<String, String>,
    next_portal: usize,
}

impl PreparedClient {
    async fn connect(server: &wire::PgwireServer) -> Self {
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect calendar pgwire fixture");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup_with_password(
            &mut reader,
            &mut writer,
            cassie_test_support::support_sql::test_root_password(),
        )
        .await;
        Self {
            reader,
            writer,
            statements: std::collections::HashMap::new(),
            next_portal: 0,
        }
    }

    async fn execute(&mut self, name: &str, query: &str, values: &[Option<&str>]) -> Frames {
        if let Some(existing) = self.statements.get(name) {
            assert_eq!(existing, query, "prepared statement SQL changed");
        }
        let mut frames = Vec::new();
        if !self.statements.contains_key(name) {
            frames.push(wire::parse_frame_with_types(name, query, &PARAMETER_OIDS));
            self.statements.insert(name.to_owned(), query.to_owned());
        }
        frames.push(wire::describe_statement_frame(name));
        let portal = format!("calendar_portal_{}", self.next_portal);
        self.next_portal += 1;
        let encoded_values = values
            .iter()
            .map(|value| value.as_ref().map(|value| value.as_bytes()))
            .collect::<Vec<_>>();
        frames.extend([
            wire::bind_frame_with_formats(&portal, name, &[0], &encoded_values, &[]),
            wire::describe_portal_frame(&portal),
            wire::execute_frame(&portal),
            wire::sync_frame(),
        ]);
        wire::write_frames(&mut self.writer, frames).await;
        wire::read_frames_until_ready_within(&mut self.reader, Duration::from_secs(10)).await
    }

    async fn simple_query(&mut self, sql: &str) -> Frames {
        wire::write_frames(&mut self.writer, vec![wire::simple_query_frame(sql)]).await;
        wire::read_frames_until_ready_within(&mut self.reader, Duration::from_secs(5)).await
    }

    async fn close(mut self) {
        wire::write_frames(&mut self.writer, vec![vec![b'X', 0, 0, 0, 4]]).await;
        let mut byte = [0];
        let read = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::AsyncReadExt::read(&mut self.reader, &mut byte),
        )
        .await
        .expect("Terminate closes the pgwire connection")
        .expect("read pgwire close");
        assert_eq!(read, 0, "Terminate should close after server-side cleanup");
    }
}

struct Range {
    label: &'static str,
    event_start: &'static str,
    event_end: &'static str,
    grid_start: &'static str,
    grid_end: &'static str,
    zone: &'static str,
}

fn runtime_config(max_result_rows: usize) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    cassie_test_support::support_sql::test_root_password().clone_into(&mut config.password);
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.query_memory_budget_bytes = 8 * 1024 * 1024;
    config.limits.max_result_rows = max_result_rows;
    config.limits.parallel_scan_workers = 1;
    config
}

fn open_local(path: &str, max_result_rows: usize) -> Cassie {
    let cassie = Cassie::new_with_data_dir_and_config(path, runtime_config(max_result_rows))
        .expect("open local dashboard store");
    cassie.startup().expect("start local dashboard store");
    cassie
}

fn open_memory(max_result_rows: usize) -> Cassie {
    let cassie = Cassie::new_in_memory_with_config(runtime_config(max_result_rows))
        .expect("open memory dashboard store");
    cassie.startup().expect("start memory dashboard store");
    cassie
}

fn create_table(cassie: &Cassie, table: &str, indexed: bool, rows: &str) {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (id INT PRIMARY KEY,device TEXT,event_at TIMESTAMPTZ,amount INT)"),
            vec![],
        )
        .expect("create dashboard source table");
    if !rows.is_empty() {
        cassie
            .execute_sql(
                &session,
                &format!("INSERT INTO {table} VALUES {rows}"),
                vec![],
            )
            .expect("seed dashboard source rows");
    }
    if indexed {
        cassie
            .execute_sql(
                &session,
                &format!(
                    "CREATE INDEX {table}_ts ON {table} USING time_series(event_at) \
                     WITH (bucket_width='1 day',partition_by='device')"
                ),
                vec![],
            )
            .expect("create fixed-UTC dashboard index");
    }
}

fn query(template: &str, table: &str) -> String {
    template.replace("{table}", table)
}

fn values(range: &Range) -> [Option<&str>; 7] {
    [
        Some("a"),
        Some("b"),
        Some(range.event_start),
        Some(range.event_end),
        Some(range.grid_start),
        Some(range.grid_end),
        Some(range.zone),
    ]
}

fn row(
    device: &str,
    local_date: &str,
    utc_boundary: &str,
    count: &str,
    total: Option<&str>,
    carried: Option<&str>,
) -> Vec<Option<String>> {
    [
        Some(device),
        Some(local_date),
        Some(utc_boundary),
        Some(count),
        total,
        carried,
    ]
    .map(|value| value.map(str::to_owned))
    .to_vec()
}

fn expected_day(range: &str) -> Vec<Vec<Option<String>>> {
    match range {
        "spring" => expected_spring_day(),
        "fall" => expected_fall_day(),
        "utc-leap" => expected_utc_leap_day(),
        _ => panic!("unknown day range {range}"),
    }
}

fn expected_spring_day() -> Vec<Vec<Option<String>>> {
    vec![
        row(
            "a",
            "2024-03-08",
            "2024-03-08 05:00:00",
            "1",
            Some("9"),
            Some("9"),
        ),
        row(
            "a",
            "2024-03-09",
            "2024-03-09 05:00:00",
            "1",
            Some("10"),
            Some("10"),
        ),
        row(
            "a",
            "2024-03-10",
            "2024-03-10 05:00:00",
            "2",
            Some("23"),
            Some("23"),
        ),
        row(
            "a",
            "2024-03-11",
            "2024-03-11 04:00:00",
            "0",
            None,
            Some("23"),
        ),
        row("a", "2024-03-12", "2024-03-12 04:00:00", "1", None, None),
        row("b", "2024-03-08", "2024-03-08 05:00:00", "0", None, None),
        row("b", "2024-03-09", "2024-03-09 05:00:00", "0", None, None),
        row(
            "b",
            "2024-03-10",
            "2024-03-10 05:00:00",
            "1",
            Some("50"),
            Some("50"),
        ),
        row(
            "b",
            "2024-03-11",
            "2024-03-11 04:00:00",
            "0",
            None,
            Some("50"),
        ),
        row(
            "b",
            "2024-03-12",
            "2024-03-12 04:00:00",
            "0",
            None,
            Some("50"),
        ),
    ]
}

fn expected_fall_day() -> Vec<Vec<Option<String>>> {
    vec![
        row("a", "2024-11-01", "2024-11-01 04:00:00", "0", None, None),
        row(
            "a",
            "2024-11-02",
            "2024-11-02 04:00:00",
            "1",
            Some("20"),
            Some("20"),
        ),
        row(
            "a",
            "2024-11-03",
            "2024-11-03 04:00:00",
            "2",
            Some("43"),
            Some("43"),
        ),
        row(
            "a",
            "2024-11-04",
            "2024-11-04 05:00:00",
            "0",
            None,
            Some("43"),
        ),
        row("b", "2024-11-01", "2024-11-01 04:00:00", "0", None, None),
        row("b", "2024-11-02", "2024-11-02 04:00:00", "0", None, None),
        row(
            "b",
            "2024-11-03",
            "2024-11-03 04:00:00",
            "1",
            Some("70"),
            Some("70"),
        ),
        row(
            "b",
            "2024-11-04",
            "2024-11-04 05:00:00",
            "0",
            None,
            Some("70"),
        ),
    ]
}

fn expected_utc_leap_day() -> Vec<Vec<Option<String>>> {
    vec![
        row(
            "a",
            "2024-02-28",
            "2024-02-28 00:00:00",
            "1",
            Some("30"),
            Some("30"),
        ),
        row(
            "a",
            "2024-02-29",
            "2024-02-29 00:00:00",
            "1",
            Some("40"),
            Some("40"),
        ),
        row(
            "a",
            "2024-03-01",
            "2024-03-01 00:00:00",
            "0",
            None,
            Some("40"),
        ),
        row("b", "2024-02-28", "2024-02-28 00:00:00", "0", None, None),
        row(
            "b",
            "2024-02-29",
            "2024-02-29 00:00:00",
            "1",
            Some("80"),
            Some("80"),
        ),
        row(
            "b",
            "2024-03-01",
            "2024-03-01 00:00:00",
            "0",
            None,
            Some("80"),
        ),
    ]
}

fn expected_empty_day(range: &str) -> Vec<Vec<Option<String>>> {
    let boundaries: &[(&str, &str)] = match range {
        "spring" => &[
            ("2024-03-08", "2024-03-08 05:00:00"),
            ("2024-03-09", "2024-03-09 05:00:00"),
            ("2024-03-10", "2024-03-10 05:00:00"),
            ("2024-03-11", "2024-03-11 04:00:00"),
            ("2024-03-12", "2024-03-12 04:00:00"),
        ],
        "fall" => &[
            ("2024-11-01", "2024-11-01 04:00:00"),
            ("2024-11-02", "2024-11-02 04:00:00"),
            ("2024-11-03", "2024-11-03 04:00:00"),
            ("2024-11-04", "2024-11-04 05:00:00"),
        ],
        "utc-leap" => &[
            ("2024-02-28", "2024-02-28 00:00:00"),
            ("2024-02-29", "2024-02-29 00:00:00"),
            ("2024-03-01", "2024-03-01 00:00:00"),
        ],
        _ => panic!("unknown empty day range {range}"),
    };
    ["a", "b"]
        .into_iter()
        .flat_map(|device| {
            boundaries
                .iter()
                .map(move |(date, utc)| row(device, date, utc, "0", None, None))
        })
        .collect()
}

fn expected_month() -> Vec<Vec<Option<String>>> {
    vec![
        row(
            "a",
            "2024-01-01",
            "2024-01-01 05:00:00",
            "2",
            Some("101"),
            Some("101"),
        ),
        row(
            "a",
            "2024-02-01",
            "2024-02-01 05:00:00",
            "1",
            Some("40"),
            Some("40"),
        ),
        row(
            "a",
            "2024-03-01",
            "2024-03-01 05:00:00",
            "1",
            Some("300"),
            Some("300"),
        ),
        row("b", "2024-01-01", "2024-01-01 05:00:00", "0", None, None),
        row(
            "b",
            "2024-02-01",
            "2024-02-01 05:00:00",
            "1",
            Some("200"),
            Some("200"),
        ),
        row(
            "b",
            "2024-03-01",
            "2024-03-01 05:00:00",
            "0",
            None,
            Some("200"),
        ),
    ]
}

fn assert_dashboard(frames: &Frames, expected: &[Vec<Option<String>>], label: &str) {
    assert_eq!(wire::error_code(frames), None, "{label}: {frames:?}");
    let parameters = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .map(|(_, payload)| wire::parse_parameter_description(payload))
        .expect("prepared statement ParameterDescription");
    assert_eq!(parameters, PARAMETER_OIDS, "{label} parameter OIDs");
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .expect("prepared portal RowDescription");
    assert_eq!(
        description
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        RESULT_OIDS,
        "{label} result OIDs"
    );
    assert_eq!(wire::data_rows(frames), expected, "{label}");
}

fn counter(metrics: &serde_json::Value, name: &str) -> u64 {
    metrics["time_series"][name].as_u64().unwrap_or_default()
}

fn create_calendar_fixture(cassie: &Cassie) -> ([Range; 3], Range) {
    let day_rows = "(1,'a','2024-03-08T06:00:00Z',9),(2,'a','2024-03-09T22:00:00Z',10), \
        (3,'a','2024-03-10T06:30:00Z',11),(4,'a','2024-03-10T07:30:00Z',12), \
        (5,'a','2024-03-12T12:00:00Z',NULL),(6,'a','2024-03-12T14:00:00Z',999), \
        (7,'a',NULL,888),(8,'b','2024-03-10T08:00:00Z',50), \
        (9,'a','2024-11-02T16:00:00Z',20),(10,'a','2024-11-03T05:30:00Z',21), \
        (11,'a','2024-11-03T06:30:00Z',22),(12,'a','2024-11-04T17:00:00Z',30), \
        (13,'b','2024-11-03T06:45:00Z',70),(14,'a','2024-02-28T00:00:00Z',5), \
        (15,'a','2024-02-28T12:00:00Z',30),(16,'a','2024-02-29T12:00:00Z',40), \
        (17,'b','2024-02-29T13:00:00Z',80),(18,'a','2024-03-01T12:00:00Z',888), \
        (19,'a','2024-03-08T05:59:59Z',999),(20,'a','2024-11-02T15:59:00Z',999)";
    for (table, indexed) in [("day_exact", false), ("day_indexed", true)] {
        create_table(cassie, table, indexed, day_rows);
    }
    let month_rows = "(1,'a','2024-01-01T06:00:00Z',1),(2,'a','2024-01-15T12:00:00Z',100), \
        (3,'a','2024-02-29T12:00:00Z',40),(4,'a','2024-03-31T21:59:59Z',300), \
        (5,'b','2024-02-29T12:00:00Z',200),(6,'a','2024-04-01T04:00:00Z',999), \
        (7,'a','2024-03-31T22:00:00Z',888),(8,'a','2024-01-01T05:00:00Z',777)";
    for (table, indexed) in [("month_exact", false), ("month_indexed", true)] {
        create_table(cassie, table, indexed, month_rows);
    }
    let ranges = [
        Range {
            label: "spring",
            event_start: "2024-03-08T06:00:00Z",
            event_end: "2024-03-12T14:00:00Z",
            grid_start: "2024-03-08T05:00:00Z",
            grid_end: "2024-03-13T04:00:00Z",
            zone: "America/New_York",
        },
        Range {
            label: "fall",
            event_start: "2024-11-02T16:00:00Z",
            event_end: "2024-11-04T17:00:00Z",
            grid_start: "2024-11-01T04:00:00Z",
            grid_end: "2024-11-05T05:00:00Z",
            zone: "America/New_York",
        },
        Range {
            label: "utc-leap",
            event_start: "2024-02-28T12:00:00Z",
            event_end: "2024-03-01T12:00:00Z",
            grid_start: "2024-02-28T00:00:00Z",
            grid_end: "2024-03-02T00:00:00Z",
            zone: "UTC",
        },
    ];
    let month_range = Range {
        label: "month",
        event_start: "2024-01-01T06:00:00Z",
        event_end: "2024-03-31T22:00:00Z",
        grid_start: "2024-01-01T05:00:00Z",
        grid_end: "2024-04-01T04:00:00Z",
        zone: "America/New_York",
    };
    (ranges, month_range)
}

fn assert_reopened_calendar_dashboard(
    path: &str,
    ranges: &[Range; 3],
    month_parameters: &[Option<&str>; 7],
) {
    // Arrange
    let reopened = open_local(path, 100_000);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("reopen runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(reopened).await;
        let mut client = PreparedClient::connect(&server).await;
        assert_eq!(
            wire::error_code(
                &client
                    .simple_query("SET TIME ZONE 'Pacific/Honolulu'")
                    .await
            ),
            None
        );
        let spring = values(&ranges[0]);
        let day = client
            .execute("reopened_day", &query(DAY_TEMPLATE, "day_indexed"), &spring)
            .await;
        let month = client
            .execute(
                "reopened_month",
                &query(MONTH_TEMPLATE, "month_indexed"),
                month_parameters,
            )
            .await;

        // Assert
        assert_dashboard(
            &day,
            &expected_day("spring"),
            "reopened explicit New York zone",
        );
        assert_dashboard(&month, &expected_month(), "reopened explicit month zone");
        client.close().await;
        server.stop().await;
    });
}

#[test]
fn should_qualify_prepared_calendar_templates() {
    // Arrange
    assert_eq!(jiff_tzdb::VERSION, Some("2026e"));
    let path = cassie_test_support::support_sql::data_dir("prepared-calendar-dashboard");
    let cassie = open_local(&path, 100_000);
    let (ranges, month_range) = create_calendar_fixture(&cassie);
    let month_parameters = values(&month_range);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(cassie.clone()).await;
        let mut client = PreparedClient::connect(&server).await;
        let exact_day_sql = query(DAY_TEMPLATE, "day_exact");
        let indexed_day_sql = query(DAY_TEMPLATE, "day_indexed");

        // Act
        for range in &ranges {
            let parameters = values(range);
            let exact = client
                .execute("day_exact", &exact_day_sql, &parameters)
                .await;
            let before = cassie.metrics();
            let indexed = client
                .execute("day_indexed", &indexed_day_sql, &parameters)
                .await;
            let after = cassie.metrics();

            // Assert
            let expected = expected_day(range.label);
            assert_dashboard(&exact, &expected, range.label);
            assert_dashboard(&indexed, &expected, range.label);
            assert_eq!(
                counter(&after, "bucket_native_hits") - counter(&before, "bucket_native_hits"),
                2
            );
            assert_eq!(
                counter(&after, "fallback_scans") - counter(&before, "fallback_scans"),
                0
            );
        }
        let mut null_event_start = values(&ranges[2]);
        null_event_start[2] = None;
        let null_bound = client
            .execute("day_null_event_start", &exact_day_sql, &null_event_start)
            .await;

        // Assert
        assert_dashboard(
            &null_bound,
            &expected_empty_day("utc-leap"),
            "NULL UTC event bound leaves calendar grid empty",
        );
        let exact_month = client
            .execute(
                "month_exact",
                &query(MONTH_TEMPLATE, "month_exact"),
                &month_parameters,
            )
            .await;
        let before = cassie.metrics();
        let indexed_month_sql = query(MONTH_TEMPLATE, "month_indexed");
        let indexed_month = client
            .execute("month_indexed", &indexed_month_sql, &month_parameters)
            .await;
        let after = cassie.metrics();

        // Assert
        assert_dashboard(&exact_month, &expected_month(), "exact month");
        assert_dashboard(&indexed_month, &expected_month(), "indexed month");
        assert_eq!(
            counter(&after, "bucket_native_hits") - counter(&before, "bucket_native_hits"),
            2
        );
        assert_eq!(
            counter(&after, "fallback_scans") - counter(&before, "fallback_scans"),
            0
        );
        // The listener does not join its detached connection worker; close it
        // before reopening the same local Midge path.
        client.close().await;
        server.stop().await;
    });
    // `support_sql::data_dir` returned a fresh UUID path, and the connection
    // worker has finished cleanup before this Midge path is opened again.
    drop(cassie);

    // Assert
    assert_reopened_calendar_dashboard(&path, &ranges, &month_parameters);
    std::fs::remove_dir_all(path).expect("remove local dashboard fixture");
}

#[test]
fn should_fail_calendar_grid_before_any_prepared_wire_rows_over_result_limit() {
    // Arrange
    let cassie = open_memory(7);
    create_table(&cassie, "calendar_limited", true, "");
    let range = Range {
        label: "eight rows",
        event_start: "2024-01-01T00:00:00Z",
        event_end: "2024-01-05T00:00:00Z",
        grid_start: "2024-01-01T00:00:00Z",
        grid_end: "2024-01-05T00:00:00Z",
        zone: "UTC",
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(cassie).await;
        let mut client = PreparedClient::connect(&server).await;

        // Act
        let frames = client
            .execute(
                "limited_calendar",
                &query(DAY_TEMPLATE, "calendar_limited"),
                &values(&range),
            )
            .await;

        // Assert
        assert_eq!(wire::error_code(&frames).as_deref(), Some("54000"));
        assert_eq!(wire::data_rows(&frames), [] as [Vec<Option<String>>; 0]);
        client.close().await;
        server.stop().await;
    });
}
