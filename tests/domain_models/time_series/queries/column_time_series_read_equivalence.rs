use super::support_read_equivalence::{sql, with_fixture};

#[test]
fn should_preserve_column_store_reads_with_time_series_indexes() {
    // Arrange
    with_fixture("column_time_series_equivalence", |cassie, session| {
        sql(cassie, session, "CREATE TABLE column_time_events (tenant TEXT,event_at TIMESTAMP,amount INT) WITH (storage = column_store)");
        sql(cassie, session, "INSERT INTO column_time_events VALUES ('a','2026-01-01T12:00:00Z',10),('a','2026-01-01T13:00:00Z',20),('b','2026-01-01T12:00:00Z',30)");
        let query = "SELECT tenant,event_at,amount FROM column_time_events WHERE tenant = 'a' AND event_at >= '2026-01-01T12:00:00Z' ORDER BY event_at LIMIT 2";
        let baseline = sql(cassie, session, query);
        sql(cassie, session, "CREATE INDEX column_time_events_idx ON column_time_events USING time_series(event_at) WITH (bucket_width = '1 hour',partition_by = tenant)");
        // Act
        let indexed = sql(cassie, session, query);
        // Assert
        assert_eq!(indexed.rows(), baseline.rows());
    });
}

use std::{collections::HashMap, time::Duration};

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::StorageFamily;
use cassie_test_support::support_pgwire as wire;
use tokio::io::AsyncReadExt;
mod support_sql {
    pub use cassie_test_support::support_sql::{
        data_dir, test_root_password, with_test_root_password,
    };

    pub fn run(
        cassie: &cassie::app::Cassie,
        session: &cassie::app::CassieSession,
        statement: &str,
        parameters: Vec<cassie::types::Value>,
    ) -> Result<cassie::executor::QueryResult, cassie::app::CassieError> {
        cassie.execute_sql(session, statement, parameters)
    }
}

const PREPARED_PAGE_OIDS: [i32; 6] = [25, 25, 1114, 1114, 20, 20];
const PREPARED_FLOAT_PAGE_OIDS: [i32; 5] = [701, 1114, 1114, 20, 20];
const PREPARED_PAGE_START: &str = "1969-12-31T23:00:00Z";
const PREPARED_PAGE_END: &str = "1970-01-01T01:00:00Z";
const PREPARED_PAGE_TABLES: [&str; 4] = ["heap_ref", "heap_idx", "col_ref", "col_idx"];

type WireFrames = Vec<(u8, Vec<u8>)>;

struct PreparedPgwireClient {
    reader: tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf,
    prepared: HashMap<String, String>,
    next_portal_id: usize,
}

impl PreparedPgwireClient {
    async fn connect(server: &wire::PgwireServer) -> Self {
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect event-time pgwire fixture");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup_with_password(
            &mut reader,
            &mut writer,
            support_sql::test_root_password(),
        )
        .await;
        Self {
            reader,
            writer,
            prepared: HashMap::new(),
            next_portal_id: 0,
        }
    }

    async fn prepared_page(
        &mut self,
        table: &str,
        tenant: &str,
        device: &str,
        limit: usize,
        offset: usize,
    ) -> WireFrames {
        let query = format!(
            "SELECT id,amount FROM {table} \
             WHERE tenant=$1 AND device=$2 AND event_at >= $3 AND event_at < $4 \
             ORDER BY event_at,id LIMIT $5 OFFSET $6"
        );
        let limit = limit.to_string();
        let offset = offset.to_string();
        let arguments = [
            tenant,
            device,
            PREPARED_PAGE_START,
            PREPARED_PAGE_END,
            limit.as_str(),
            offset.as_str(),
        ];
        self.prepared_query(table, &query, &PREPARED_PAGE_OIDS, &arguments)
            .await
    }

    async fn prepared_query(
        &mut self,
        statement_name: &str,
        query: &str,
        parameter_oids: &[i32],
        parameters: &[&str],
    ) -> WireFrames {
        if let Some(prepared_query) = self.prepared.get(statement_name) {
            assert_eq!(prepared_query, query, "prepared statement SQL changed");
        }
        let mut frames = Vec::new();
        if !self.prepared.contains_key(statement_name) {
            frames.push(wire::parse_frame_with_types(
                statement_name,
                query,
                parameter_oids,
            ));
            self.prepared
                .insert(statement_name.to_owned(), query.to_owned());
        }
        let portal_name = format!("time_series_portal_{}", self.next_portal_id);
        self.next_portal_id += 1;
        frames.extend([
            wire::bind_frame(&portal_name, statement_name, parameters),
            wire::describe_portal_frame(&portal_name),
            wire::execute_frame(&portal_name),
            wire::sync_frame(),
        ]);
        wire::write_frames(&mut self.writer, frames).await;
        wire::read_frames_until_ready_within(&mut self.reader, Duration::from_secs(5)).await
    }

    async fn suspended_page(
        &mut self,
        table: &str,
        tenant: &str,
        device: &str,
        max_rows: i32,
    ) -> (String, WireFrames) {
        let statement_name = format!("suspended_{table}");
        let query = format!(
            "SELECT id,amount FROM {table} \
             WHERE tenant=$1 AND device=$2 AND event_at >= $3 AND event_at < $4 \
             ORDER BY event_at,id LIMIT $5 OFFSET $6"
        );
        let limit = "99";
        let offset = "0";
        let parameters = [
            tenant,
            device,
            PREPARED_PAGE_START,
            PREPARED_PAGE_END,
            limit,
            offset,
        ];
        let portal_name = format!("suspended_portal_{}", self.next_portal_id);
        self.next_portal_id += 1;
        let frames = vec![
            wire::parse_frame_with_types(&statement_name, &query, &PREPARED_PAGE_OIDS),
            wire::bind_frame(&portal_name, &statement_name, &parameters),
            wire::describe_portal_frame(&portal_name),
            wire::execute_limited_frame(&portal_name, max_rows),
        ];
        wire::write_frames(&mut self.writer, frames).await;
        let frames = tokio::time::timeout(Duration::from_secs(5), async {
            let mut response = Vec::new();
            loop {
                let frame = wire::read_wire_frame(&mut self.reader).await;
                let is_suspended = frame.0 == b's';
                response.push(frame);
                if is_suspended {
                    break;
                }
            }
            response
        })
        .await
        .expect("portal should suspend after its row limit");
        (portal_name, frames)
    }

    async fn resume_portal(&mut self, portal_name: &str) -> WireFrames {
        wire::write_frames(
            &mut self.writer,
            vec![wire::execute_frame(portal_name), wire::sync_frame()],
        )
        .await;
        wire::read_frames_until_ready_within(&mut self.reader, Duration::from_secs(5)).await
    }

    async fn simple_query(&mut self, statement: &str) -> WireFrames {
        wire::write_frames(&mut self.writer, vec![wire::simple_query_frame(statement)]).await;
        wire::read_frames_until_ready_within(&mut self.reader, Duration::from_secs(5)).await
    }

    async fn terminate(&mut self) {
        wire::write_frames(&mut self.writer, vec![vec![b'X', 0, 0, 0, 4]]).await;
        let mut byte = [0];
        let read = tokio::time::timeout(Duration::from_secs(5), self.reader.read(&mut byte))
            .await
            .expect("Terminate should finish server-side session cleanup")
            .expect("read closed pgwire connection");
        assert_eq!(read, 0, "Terminate should close the pgwire connection");
    }
}

fn prepared_fixture(memory_budget: usize) -> Cassie {
    let mut config = support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    cassie
}

fn prepared_local_fixture(data_dir: &str, memory_budget: usize, max_result_rows: usize) -> Cassie {
    let mut config = support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.max_result_rows = max_result_rows;
    config.limits.parallel_scan_workers = 1;
    let cassie =
        Cassie::new_with_data_dir_and_config(data_dir, config).expect("create local Cassie");
    cassie.startup().expect("start Cassie");
    cassie
}

fn seed_prepared_page_twins(cassie: &Cassie) {
    let session = cassie.create_session("tester", None);
    for table in PREPARED_PAGE_TABLES {
        let storage = if table.starts_with("col_") {
            " WITH (storage = column_store)"
        } else {
            ""
        };
        support_sql::run(
            cassie,
            &session,
            &format!(
                "CREATE TABLE {table} (id INT PRIMARY KEY, tenant TEXT, device TEXT, \
                 event_at TIMESTAMP, amount INT){storage}"
            ),
            vec![],
        )
        .expect("create heap or column-store twin");
        support_sql::run(
            cassie,
            &session,
            &format!(
                "INSERT INTO {table} VALUES \
                 (1,'a','bc','1969-12-31T23:00:00Z',10), \
                 (2,'a','bc','1969-12-31T23:00:00Z',20), \
                 (3,'a','bc','1969-12-31T23:15:00Z',30), \
                 (4,'a','bc','1970-01-01T00:00:00Z',40), \
                 (5,'a','bc','1970-01-01T01:00:00Z',50), \
                 (6,'a','bc','1969-12-31T22:59:59Z',60), \
                 (11,'ab','c','1969-12-31T23:00:00Z',110), \
                 (12,'ab','c','1970-01-01T00:00:00Z',120)"
            ),
            vec![],
        )
        .expect("seed half-open range and colliding composite keys");
        if table.ends_with("_idx") {
            support_sql::run(
                cassie,
                &session,
                &format!(
                    "CREATE INDEX {table}_ts ON {table} USING time_series(event_at) \
                     WITH (bucket_width='1 hour',partition_by='tenant,device')"
                ),
                vec![],
            )
            .expect("create time-series index");
        }
    }
}

fn prepared_page_rows(frames: &WireFrames) -> Vec<Vec<Option<String>>> {
    assert_eq!(
        wire::error_code(frames),
        None,
        "prepared page error: {frames:?}"
    );
    wire::data_rows(frames)
}

fn expected_prepared_page(tenant: &str, device: &str) -> Vec<Vec<Option<String>>> {
    let rows = match (tenant, device) {
        ("a", "bc") => vec![("1", "10"), ("2", "20"), ("3", "30"), ("4", "40")],
        ("ab", "c") => vec![("11", "110"), ("12", "120")],
        _ => Vec::new(),
    };
    rows.into_iter()
        .map(|(id, amount)| vec![Some(id.to_owned()), Some(amount.to_owned())])
        .collect()
}

fn expected_corrected_page(
    tenant: &str,
    device: &str,
    corrected_amount: &str,
) -> Vec<Vec<Option<String>>> {
    let rows = match (tenant, device) {
        ("a", "bc") => vec![
            ("1", "10"),
            ("2", "20"),
            ("7", "70"),
            ("4", corrected_amount),
        ],
        ("ab", "c") => vec![("11", "110"), ("12", "120"), ("3", "330")],
        _ => Vec::new(),
    };
    rows.into_iter()
        .map(|(id, amount)| vec![Some(id.to_owned()), Some(amount.to_owned())])
        .collect()
}

fn time_series_counter(metrics: &serde_json::Value, name: &str) -> u64 {
    metrics["time_series"][name].as_u64().unwrap_or_default()
}

fn corrupt_prepared_membership(cassie: &Cassie, table: &str, id: &str) {
    let collection = cassie
        .catalog
        .get_schema(table)
        .expect("table metadata")
        .collection;
    let index = cassie
        .catalog
        .get_index(&collection, &format!("{table}_ts"))
        .expect("time-series index metadata");
    let prefix = cassie
        .midge
        .time_series_index_prefix_for_diagnostics(&collection, &index.name)
        .expect("time-series membership prefix");
    let session = cassie.create_session("time-series-corruption", None);
    let selected_row = support_sql::run(
        cassie,
        &session,
        &format!("SELECT _id FROM {table} WHERE id = {id}"),
        vec![],
    )
    .expect("look up physical document identity");
    assert_eq!(selected_row.rows().len(), 1, "one selected SQL primary key");
    let row_id = selected_row.rows()[0][0]
        .as_str()
        .expect("physical document identity is text")
        .to_owned();
    // `append_terminated_component` stores the final Midge document id as UTF-8 + 0x00.
    let row_id_suffix = [row_id.as_bytes(), &[0]].concat();
    let (key, _) = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("scan time-series membership")
        .into_iter()
        .find(|(key, _)| key.ends_with(&row_id_suffix))
        .expect("selected time-series membership");
    cassie
        .midge
        .raw_delete(StorageFamily::Data, &key)
        .expect("remove one selected membership");
}

#[test]
fn should_use_columnar_bucket_hits_for_prepared_half_open_pages() {
    // Arrange
    let cassie = prepared_fixture(8 * 1024 * 1024);
    seed_prepared_page_twins(&cassie);
    let observed = cassie.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(cassie.clone()).await;
        let mut client = PreparedPgwireClient::connect(&server).await;

        // Act
        for table in PREPARED_PAGE_TABLES {
            for (tenant, device) in [("a", "bc"), ("ab", "c")] {
                for (limit, offset) in [(2, 0), (2, 2), (99, 0)] {
                    let frames = client
                        .prepared_page(table, tenant, device, limit, offset)
                        .await;
                    let expected = expected_prepared_page(tenant, device);
                    let start = offset.min(expected.len());
                    let end = start.saturating_add(limit).min(expected.len());
                    assert_eq!(prepared_page_rows(&frames), expected[start..end], "{table}");
                }
            }
        }

        // Assert
        let before = observed.metrics();
        let indexed = client.prepared_page("col_idx", "a", "bc", 99, 0).await;
        let after = observed.metrics();
        assert_eq!(
            prepared_page_rows(&indexed),
            expected_prepared_page("a", "bc")
        );
        assert_eq!(
            time_series_counter(&after, "bucket_native_hits")
                .saturating_sub(time_series_counter(&before, "bucket_native_hits")),
            1,
            "a healthy indexed column-store query must use native bucket reads"
        );
        assert_eq!(
            time_series_counter(&after, "fallback_scans")
                .saturating_sub(time_series_counter(&before, "fallback_scans")),
            0
        );
        let selected = [
            "buckets_scanned",
            "index_entries_scanned",
            "row_point_fetches",
        ]
        .map(|name| time_series_counter(&after, name) - time_series_counter(&before, name));

        let setup = observed.create_session("tester", None);
        support_sql::run(
            &observed,
            &setup,
            "INSERT INTO col_idx VALUES \
             (21,'a','bc','1970-01-02T00:00:00Z',210), \
             (22,'different','device','1970-01-01T00:00:00Z',220)",
            vec![],
        )
        .expect("insert rows outside the selected partition and time range");
        let before_noise_query = observed.metrics();

        // Act
        let after_noise = client.prepared_page("col_idx", "a", "bc", 99, 0).await;
        let after_noise_metrics = observed.metrics();

        // Assert
        assert_eq!(
            prepared_page_rows(&after_noise),
            expected_prepared_page("a", "bc")
        );
        assert_eq!(
            time_series_counter(&after_noise_metrics, "bucket_native_hits").saturating_sub(
                time_series_counter(&before_noise_query, "bucket_native_hits")
            ),
            1
        );
        let selected_after = [
            "buckets_scanned",
            "index_entries_scanned",
            "row_point_fetches",
        ]
        .map(|name| {
            time_series_counter(&after_noise_metrics, name)
                - time_series_counter(&before_noise_query, name)
        });
        assert_eq!(selected_after, selected);
        server.stop().await;
    });
}

#[test]
fn should_fallback_to_complete_columnar_page_given_missing_bucket_membership() {
    // Arrange
    let cassie = prepared_fixture(8 * 1024 * 1024);
    seed_prepared_page_twins(&cassie);
    corrupt_prepared_membership(&cassie, "col_idx", "2");
    let observed = cassie.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(cassie).await;
        let mut client = PreparedPgwireClient::connect(&server).await;

        // Act
        let frames = client.prepared_page("col_idx", "a", "bc", 99, 0).await;
        let metrics = observed.metrics();

        // Assert
        assert_eq!(
            prepared_page_rows(&frames),
            expected_prepared_page("a", "bc"),
            "the incomplete native candidate set must not become a partial page"
        );
        assert_eq!(
            metrics["time_series"]["last_fallback_reason"].as_str(),
            Some("incomplete-bucket-membership")
        );
        assert_eq!(time_series_counter(&metrics, "fallback_scans"), 1);
        server.stop().await;
    });

    // Arrange
    // Hypothesis: 4 KiB admits fixture setup but not the 48-row authoritative fallback.
    let cassie = prepared_fixture(4 * 1024);
    seed_prepared_page_twins(&cassie);
    let session = cassie.create_session("tester", None);
    for id in 100..140 {
        support_sql::run(
            &cassie,
            &session,
            &format!("INSERT INTO col_idx VALUES ({id},'a','bc','1970-02-01T00:00:00Z',{id})"),
            vec![],
        )
        .expect("seed a bounded page with an over-budget authoritative fallback");
    }
    corrupt_prepared_membership(&cassie, "col_idx", "2");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(cassie).await;
        let mut client = PreparedPgwireClient::connect(&server).await;

        // Act
        let frames = client.prepared_page("col_idx", "a", "bc", 99, 0).await;

        // Assert
        assert_eq!(wire::error_code(&frames).as_deref(), Some("54000"));
        assert!(
            wire::data_rows(&frames).is_empty(),
            "a resource failure must not publish partial DataRow frames"
        );
        server.stop().await;
    });
}

fn apply_prepared_corrections(cassie: &Cassie, setup: &cassie::app::CassieSession) {
    for table in PREPARED_PAGE_TABLES {
        support_sql::run(
            cassie,
            setup,
            &format!(
                "INSERT INTO {table} VALUES \
                 (7,'a','bc','1969-12-31T23:10:00Z',70)"
            ),
            vec![],
        )
        .expect("insert a late event");
        support_sql::run(
            cassie,
            setup,
            &format!(
                "UPDATE {table} SET tenant='ab', device='c', \
                 event_at='1970-01-01T00:15:00Z', amount=330 WHERE id=3"
            ),
            vec![],
        )
        .expect("move one event across partition and bucket");
    }
}

async fn assert_prepared_corrected_pages(client: &mut PreparedPgwireClient) {
    for table in PREPARED_PAGE_TABLES {
        for (tenant, device) in [("a", "bc"), ("ab", "c")] {
            let frames = client.prepared_page(table, tenant, device, 99, 0).await;
            let expected = expected_corrected_page(tenant, device, "40");
            assert_eq!(prepared_page_rows(&frames), expected, "{table}");
        }
    }
}

async fn assert_staged_correction_rollback(client: &mut PreparedPgwireClient) {
    assert_eq!(wire::error_code(&client.simple_query("BEGIN").await), None);
    let update = client
        .simple_query("UPDATE col_idx SET amount=777 WHERE id=4")
        .await;
    assert_eq!(wire::error_code(&update), None);
    let staged = client.prepared_page("col_idx", "a", "bc", 99, 0).await;
    assert_eq!(
        prepared_page_rows(&staged),
        expected_corrected_page("a", "bc", "777"),
        "the writing session must see its staged correction"
    );
    assert_eq!(
        wire::error_code(&client.simple_query("ROLLBACK").await),
        None
    );
    let rolled_back = client.prepared_page("col_idx", "a", "bc", 99, 0).await;
    assert_eq!(
        prepared_page_rows(&rolled_back),
        expected_corrected_page("a", "bc", "40"),
        "rollback must restore the authoritative source and bucket membership"
    );
}

async fn assert_suspended_portal_snapshot(
    cassie: &Cassie,
    setup: &cassie::app::CassieSession,
    client: &mut PreparedPgwireClient,
) {
    let (portal_name, first_page) = client.suspended_page("col_idx", "a", "bc", 2).await;
    assert_eq!(wire::error_code(&first_page), None);
    assert_eq!(
        wire::data_rows(&first_page),
        expected_corrected_page("a", "bc", "40")[..2]
    );
    assert!(first_page.iter().any(|(tag, _)| *tag == b's'));
    support_sql::run(
        cassie,
        setup,
        "UPDATE col_idx SET amount=888 WHERE id=4",
        vec![],
    )
    .expect("commit a source update while the portal is suspended");

    let resumed = client.resume_portal(&portal_name).await;
    assert_eq!(wire::error_code(&resumed), None);
    assert_eq!(
        wire::data_rows(&resumed),
        expected_corrected_page("a", "bc", "40")[2..],
        "the suspended portal must retain its statement snapshot"
    );
    let after_portal = client.prepared_page("col_idx", "a", "bc", 99, 0).await;
    assert_eq!(
        prepared_page_rows(&after_portal),
        expected_corrected_page("a", "bc", "888"),
        "a new statement must observe the committed source update"
    );
}

fn run_prepared_correction_portal_checks(cassie: &Cassie, setup: &cassie::app::CassieSession) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(cassie.clone()).await;
        let mut client = PreparedPgwireClient::connect(&server).await;
        apply_prepared_corrections(cassie, setup);
        assert_prepared_corrected_pages(&mut client).await;
        assert_staged_correction_rollback(&mut client).await;
        assert_suspended_portal_snapshot(cassie, setup, &mut client).await;
        client.terminate().await;
        drop(client);
        server.stop().await;
    });
}

async fn assert_reopened_prepared_pages(client: &mut PreparedPgwireClient) {
    for table in PREPARED_PAGE_TABLES {
        let frames = client.prepared_page(table, "a", "bc", 99, 0).await;
        let expected_amount = if table == "col_idx" { "888" } else { "40" };
        assert_eq!(
            prepared_page_rows(&frames),
            expected_corrected_page("a", "bc", expected_amount),
            "reopened table {table}"
        );
    }
}

fn assert_prepared_local_reopen(path: &str) {
    let restarted = prepared_local_fixture(path, 8 * 1024 * 1024, 100_000);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("reopen runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(restarted.clone()).await;
        let mut client = PreparedPgwireClient::connect(&server).await;
        assert_reopened_prepared_pages(&mut client).await;
        client.terminate().await;
        drop(client);
        server.stop().await;
    });
    drop(runtime);
    drop(restarted);
}

#[test]
fn should_preserve_prepared_columnar_reads_across_correction_rollback_portal_and_reopen() {
    // Arrange
    let path = support_sql::data_dir("prepared-columnar-time-series-reopen");
    let cassie = prepared_local_fixture(&path, 8 * 1024 * 1024, 100_000);
    seed_prepared_page_twins(&cassie);
    let setup = cassie.create_session("tester", None);

    // Act
    run_prepared_correction_portal_checks(&cassie, &setup);
    drop(setup);
    drop(cassie);

    // Assert
    assert_prepared_local_reopen(&path);
    std::fs::remove_dir_all(path).expect("remove local time-series fixture");
}

fn seed_signed_zero_tables(cassie: &Cassie, session: &cassie::app::CassieSession, tables: &[&str]) {
    for table in tables {
        let storage = if table.starts_with("f_col_") {
            " WITH (storage = column_store)"
        } else {
            ""
        };
        support_sql::run(
            cassie,
            session,
            &format!(
                "CREATE TABLE {table} (id INT PRIMARY KEY, device FLOAT, \
                 event_at TIMESTAMP, amount INT){storage}"
            ),
            vec![],
        )
        .expect("create FLOAT signed-zero table");
        support_sql::run(
            cassie,
            session,
            &format!(
                "INSERT INTO {table} VALUES \
                 (1,1.5,'1969-12-31T23:00:00Z',10), \
                 (2,1.5,'1970-01-01T00:00:00Z',20), \
                 (3,0.0,'1970-01-01T00:00:00Z',30), \
                 (4,-0.0,'1970-01-01T00:00:00Z',40)"
            ),
            vec![],
        )
        .expect("seed signed-zero partition spellings");
        if table.ends_with("_idx") {
            support_sql::run(
                cassie,
                session,
                &format!(
                    "CREATE INDEX {table}_ts ON {table} USING time_series(event_at) \
                     WITH (bucket_width='1 hour',partition_by='device')"
                ),
                vec![],
            )
            .expect("create FLOAT time-series index");
        }
    }
}

fn insert_signed_zero_noise(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    tables: &[&str],
) {
    for table in tables {
        support_sql::run(
            cassie,
            session,
            &format!(
                "INSERT INTO {table} VALUES \
                 (5,9.0,'1970-01-01T00:00:00Z',50), \
                 (6,1.5,'1970-01-04T00:00:00Z',60)"
            ),
            vec![],
        )
        .expect("insert nonzero and out-of-range noise rows");
    }
}

fn signed_zero_query(table: &str) -> String {
    format!(
        "SELECT id,amount FROM {table} WHERE device=$1 \
         AND event_at >= $2 AND event_at < $3 ORDER BY event_at,id \
         LIMIT $4 OFFSET $5"
    )
}

fn expected_signed_zero_rows(partition: &str) -> Vec<Vec<Option<String>>> {
    let values = if partition == "1.5" {
        [("1", "10"), ("2", "20")]
    } else {
        [("3", "30"), ("4", "40")]
    };
    values
        .into_iter()
        .map(|(id, amount)| vec![Some(id.to_owned()), Some(amount.to_owned())])
        .collect()
}

fn assert_signed_zero_index_counters(
    before: &serde_json::Value,
    after: &serde_json::Value,
    table: &str,
    partition: &str,
    after_noise: bool,
) {
    if !table.ends_with("_idx") {
        return;
    }
    assert_eq!(
        time_series_counter(after, "bucket_native_hits")
            - time_series_counter(before, "bucket_native_hits"),
        1,
        "indexed FLOAT query {table} {partition}"
    );
    assert_eq!(
        time_series_counter(after, "fallback_scans")
            - time_series_counter(before, "fallback_scans"),
        0
    );
    let expected_candidates = match (partition, after_noise) {
        ("1.5", _) => 2,
        (_, false) => 4,
        (_, true) => 6,
    };
    let expected_point_fetches = match (partition, after_noise) {
        ("1.5", _) => 2,
        (_, false) => 4,
        (_, true) => 5,
    };
    assert_eq!(
        time_series_counter(after, "index_entries_scanned")
            - time_series_counter(before, "index_entries_scanned"),
        expected_candidates,
        "preserve the signed-zero broad-candidate exception for {table}"
    );
    assert_eq!(
        time_series_counter(after, "row_point_fetches")
            - time_series_counter(before, "row_point_fetches"),
        expected_point_fetches,
        "out-of-range membership is counted as scanned but pruned before row fetch"
    );
}

async fn assert_signed_zero_prepared_query(
    client: &mut PreparedPgwireClient,
    observed: &Cassie,
    table: &str,
    partition: &str,
    after_noise: bool,
) {
    let before = observed.metrics();
    let query = signed_zero_query(table);
    let frames = client
        .prepared_query(
            &format!("signed_zero_{table}"),
            &query,
            &PREPARED_FLOAT_PAGE_OIDS,
            &[partition, PREPARED_PAGE_START, PREPARED_PAGE_END, "9", "0"],
        )
        .await;
    let after = observed.metrics();

    assert_eq!(wire::error_code(&frames), None, "{table} {partition}");
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .expect("prepared FLOAT result description");
    assert_eq!(
        description
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(&frames),
        expected_signed_zero_rows(partition),
        "{table} {partition}"
    );
    assert_signed_zero_index_counters(&before, &after, table, partition, after_noise);
}

fn run_signed_zero_prepared_probes(
    cassie: &Cassie,
    observed: &Cassie,
    session: &cassie::app::CassieSession,
    tables: &[&str],
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(cassie.clone()).await;
        let mut client = PreparedPgwireClient::connect(&server).await;

        for after_noise in [false, true] {
            if after_noise {
                insert_signed_zero_noise(cassie, session, tables);
            }
            for table in tables {
                for partition in ["1.5", "-0.0", "0.0"] {
                    assert_signed_zero_prepared_query(
                        &mut client,
                        observed,
                        table,
                        partition,
                        after_noise,
                    )
                    .await;
                }
            }
        }
        client.terminate().await;
        drop(client);
        server.stop().await;
    });
}

#[test]
fn should_preserve_signed_zero_candidate_bounds_through_prepared_columnar_reads() {
    // Arrange
    let cassie = prepared_fixture(8 * 1024 * 1024);
    let session = cassie.create_session("tester", None);
    let tables = ["f_heap_ref", "f_heap_idx", "f_col_ref", "f_col_idx"];
    seed_signed_zero_tables(&cassie, &session, &tables);
    let observed = cassie.clone();

    // Act
    run_signed_zero_prepared_probes(&cassie, &observed, &session, &tables);

    // Assert
    // Each prepared probe asserts rows, result types, and indexed candidate deltas.
}
