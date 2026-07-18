use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};
use serde_json::json;
use uuid::Uuid;

#[path = "support/pgwire.rs"]
mod wire;

const FIXTURE_ROWS: usize = 64;
const RESULT_LIMIT: usize = 5;
const LOW_MEMORY_BYTES: usize = 1_024;
const NORMAL_MEMORY_BUDGET: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
enum AnalyticalFamily {
    TimeSeries,
    ColumnProjection,
    ColumnAggregate,
    Graph,
}

impl AnalyticalFamily {
    const ALL: [Self; 4] = [
        Self::TimeSeries,
        Self::ColumnProjection,
        Self::ColumnAggregate,
        Self::Graph,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::TimeSeries => "time-series",
            Self::ColumnProjection => "column-projection",
            Self::ColumnAggregate => "column-aggregate",
            Self::Graph => "graph",
        }
    }

    const fn query(self) -> &'static str {
        match self {
            Self::TimeSeries => {
                "SELECT id, amount FROM controlled_time_series WHERE event_at >= '2026-01-01T00:00:00Z' LIMIT 5"
            }
            Self::ColumnProjection => {
                "SELECT id, score, label FROM controlled_column_projection WHERE score >= 0 LIMIT 5"
            }
            Self::ColumnAggregate => {
                "SELECT COUNT(*), SUM(score), AVG(score) FROM controlled_column_aggregate"
            }
            Self::Graph => {
                "SELECT edge_id, node_id, cost FROM graph_neighbors('controlled_graph', 'person', 'root', 'out', 'knows', 5) LIMIT 5"
            }
        }
    }

    fn successful_paths(self, metrics: &serde_json::Value) -> u64 {
        match self {
            Self::TimeSeries => metrics["time_series"]["scans"].as_u64().unwrap_or_default(),
            Self::ColumnProjection => metrics["column_batches"]["scans"]
                .as_u64()
                .unwrap_or_default(),
            Self::ColumnAggregate => metrics["aggregate_acceleration"]["scans"]
                .as_u64()
                .unwrap_or_default(),
            Self::Graph => metrics["graph"]["traversals"].as_u64().unwrap_or_default(),
        }
    }

    const fn controlled_read_bound(self) -> u64 {
        match self {
            Self::TimeSeries => (3 * FIXTURE_ROWS + 1) as u64,
            Self::ColumnProjection | Self::ColumnAggregate | Self::Graph => FIXTURE_ROWS as u64,
        }
    }
}

struct Fixture {
    cassie: Cassie,
    session: CassieSession,
    path: String,
}

struct FallbackEvidence {
    rows: Vec<Vec<cassie::types::Value>>,
    overlay_metrics: Option<serde_json::Value>,
    final_metrics: serde_json::Value,
}

impl Fixture {
    fn new(family: AnalyticalFamily, memory_budget: usize) -> Self {
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
        let path = std::env::temp_dir()
            .join(format!(
                "cassie-specialized-analytical-{}-{}",
                family.label(),
                Uuid::new_v4()
            ))
            .to_string_lossy()
            .into_owned();
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = memory_budget;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        seed_family(&cassie, &session, family);
        Self {
            cassie,
            session,
            path,
        }
    }

    fn cleanup(self) {
        drop(self.cassie);
        let _ = std::fs::remove_dir_all(self.path);
    }
}

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
    fields
        .iter()
        .find(|(field, _)| *field == tag)
        .map(|(_, value)| value.as_str())
}

fn wire_sqlstate(
    family: AnalyticalFamily,
    memory_budget: usize,
    cancel_after_reads: Option<usize>,
) -> String {
    let fixture = Fixture::new(family, memory_budget);
    let Fixture {
        cassie,
        session,
        path,
    } = fixture;
    drop(session);
    let runtime = current_thread_runtime();
    let sqlstate = runtime.block_on(async {
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        set_query_scan_cancellation_after_entries(cancel_after_reads);
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(family.query())]).await;
        let frames = wire::read_frames_until_ready(&mut reader).await;
        set_query_scan_cancellation_after_entries(None);
        let error = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("pgwire analytical error");
        let fields = wire::parse_error_fields(&error.1);
        let sqlstate = error_field(&fields, 'C')
            .expect("SQLSTATE error field")
            .to_string();
        server.stop().await;
        sqlstate
    });
    let _ = std::fs::remove_dir_all(path);
    sqlstate
}

fn seed_family(cassie: &Cassie, session: &CassieSession, family: AnalyticalFamily) {
    match family {
        AnalyticalFamily::TimeSeries => seed_time_series(cassie, session),
        AnalyticalFamily::ColumnProjection => {
            seed_column(cassie, session, "controlled_column_projection");
        }
        AnalyticalFamily::ColumnAggregate => {
            seed_column(cassie, session, "controlled_column_aggregate");
        }
        AnalyticalFamily::Graph => seed_graph(cassie, session),
    }
}

fn seed_time_series(cassie: &Cassie, session: &CassieSession) {
    execute(
        cassie,
        session,
        "CREATE TABLE controlled_time_series (tenant TEXT, event_at TIMESTAMP, amount INT)",
    );
    execute(
        cassie,
        session,
        "CREATE INDEX controlled_time_series_idx ON controlled_time_series USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
    );
    let rows = (0..FIXTURE_ROWS)
        .map(|index| {
            let day = 1 + index / 24;
            let hour = index % 24;
            (
                Some(format!("event-{index:04}")),
                json!({
                    "tenant": "acme",
                    "event_at": format!("2026-01-{day:02}T{hour:02}:00:00Z"),
                    "amount": index,
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_time_series_documents("controlled_time_series", rows)
        .expect("seed time-series rows");
}

fn seed_column(cassie: &Cassie, session: &CassieSession, table: &str) {
    execute(
        cassie,
        session,
        &format!("CREATE TABLE {table} (score INT, label TEXT)"),
    );
    let rows = (0..FIXTURE_ROWS)
        .map(|index| {
            (
                Some(format!("row-{index:04}")),
                json!({
                    "score": index,
                    "label": format!("label-{index:04}-{}", "x".repeat(256)),
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(table, rows)
        .expect("seed column rows");
    execute(
        cassie,
        session,
        &format!(
            "CREATE INDEX {table}_idx ON {table} USING column (score, label) WITH (segment_size = 1)"
        ),
    );
}

fn seed_graph(cassie: &Cassie, session: &CassieSession) {
    execute(cassie, session, "CREATE GRAPH controlled_graph");
    let rows = (0..FIXTURE_ROWS)
        .map(|index| {
            let edge_id = format!("edge-{index:04}");
            (
                Some(edge_id.clone()),
                json!({
                    "edge_id": edge_id,
                    "source_type": "person",
                    "source_id": "root",
                    "target_type": "person",
                    "target_id": format!("node-{index:04}-{}", "x".repeat(256)),
                    "edge_type": "knows",
                    "weight": index,
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_graph_documents("controlled_graph_edges", rows)
        .expect("seed graph edges");
}

fn metric(metrics: &serde_json::Value, family: &str, name: &str) -> u64 {
    metrics[family][name].as_u64().unwrap_or_default()
}

fn assert_query_cleanup(cassie: &Cassie) {
    let metrics = cassie.metrics();
    assert_eq!(metric(&metrics, "runtime", "running_queries"), 0);
    assert_eq!(
        metric(&metrics, "query", "current_accounted_memory_bytes"),
        0
    );
}

fn assert_failed_path_metrics_unchanged(
    family: AnalyticalFamily,
    before: &serde_json::Value,
    after: &serde_json::Value,
) {
    let fields: &[(&str, &str)] = match family {
        AnalyticalFamily::TimeSeries => &[
            ("time_series", "scans"),
            ("time_series", "bucket_native_hits"),
            ("time_series", "fallback_scans"),
            ("time_series", "rows"),
            ("time_series", "index_entries_scanned"),
            ("time_series", "row_point_fetches"),
        ],
        AnalyticalFamily::ColumnProjection => &[
            ("column_batches", "scans"),
            ("column_batches", "row_fetches_avoided"),
            ("column_batches", "fallback_scans"),
            ("column_batches", "decoded_columns"),
        ],
        AnalyticalFamily::ColumnAggregate => &[
            ("aggregate_acceleration", "scans"),
            ("aggregate_acceleration", "accelerated_segments"),
            ("aggregate_acceleration", "row_blob_fallbacks"),
            ("column_batches", "fallback_scans"),
        ],
        AnalyticalFamily::Graph => &[
            ("graph", "traversals"),
            ("graph", "rows"),
            ("graph", "reads"),
            ("graph", "candidates"),
        ],
    };
    for (metric_family, name) in fields {
        assert_eq!(
            metric(after, metric_family, name),
            metric(before, metric_family, name),
            "{} published failed-path metric {metric_family}.{name}",
            family.label()
        );
    }
    assert_eq!(
        metric(after, "query", "rows_returned_total"),
        metric(before, "query", "rows_returned_total"),
        "{} published partial rows",
        family.label()
    );
}

fn exact_fallback_evidence(family: AnalyticalFamily, fixture: &Fixture) -> FallbackEvidence {
    match family {
        AnalyticalFamily::TimeSeries => execute(
            &fixture.cassie,
            &fixture.session,
            "DROP INDEX controlled_time_series_idx ON controlled_time_series",
        ),
        AnalyticalFamily::ColumnProjection => execute(
            &fixture.cassie,
            &fixture.session,
            "DROP INDEX controlled_column_projection_idx ON controlled_column_projection",
        ),
        AnalyticalFamily::ColumnAggregate => execute(
            &fixture.cassie,
            &fixture.session,
            "DROP INDEX controlled_column_aggregate_idx ON controlled_column_aggregate",
        ),
        AnalyticalFamily::Graph => {
            execute(&fixture.cassie, &fixture.session, "BEGIN");
            execute(
                &fixture.cassie,
                &fixture.session,
                "INSERT INTO controlled_graph_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('edge-extra', 'person', 'root', 'person', 'node-extra', 'knows', 0)",
            );
        }
    }
    let rows = fixture
        .cassie
        .execute_sql(&fixture.session, family.query(), vec![])
        .expect("exact controlled fallback")
        .rows;
    if matches!(family, AnalyticalFamily::Graph) {
        let overlay_metrics = fixture.cassie.metrics();
        execute(&fixture.cassie, &fixture.session, "COMMIT");
        let committed = fixture
            .cassie
            .execute_sql(&fixture.session, family.query(), vec![])
            .expect("committed native graph query")
            .rows;
        assert_eq!(rows, committed, "graph overlay/native equivalence");
        return FallbackEvidence {
            rows,
            overlay_metrics: Some(overlay_metrics),
            final_metrics: fixture.cassie.metrics(),
        };
    }
    FallbackEvidence {
        rows,
        overlay_metrics: None,
        final_metrics: fixture.cassie.metrics(),
    }
}

fn assert_success_metrics(
    family: AnalyticalFamily,
    before: &serde_json::Value,
    after: &serde_json::Value,
) {
    assert_eq!(
        family.successful_paths(after) - family.successful_paths(before),
        2,
        "{} successful path count",
        family.label()
    );
    match family {
        AnalyticalFamily::TimeSeries => {
            assert_eq!(
                metric(after, "time_series", "bucket_native_hits")
                    - metric(before, "time_series", "bucket_native_hits"),
                2
            );
            assert!(
                metric(after, "time_series", "index_entries_scanned")
                    - metric(before, "time_series", "index_entries_scanned")
                    <= (2 * FIXTURE_ROWS) as u64
            );
            assert!(
                metric(after, "time_series", "row_point_fetches")
                    - metric(before, "time_series", "row_point_fetches")
                    <= (2 * FIXTURE_ROWS) as u64
            );
        }
        AnalyticalFamily::ColumnProjection => {
            assert_eq!(
                metric(after, "column_batches", "row_fetches_avoided")
                    - metric(before, "column_batches", "row_fetches_avoided"),
                (2 * FIXTURE_ROWS) as u64
            );
            assert!(
                metric(after, "column_batches", "decoded_columns")
                    - metric(before, "column_batches", "decoded_columns")
                    <= (2 * FIXTURE_ROWS * 2) as u64
            );
        }
        AnalyticalFamily::ColumnAggregate => assert_eq!(
            metric(after, "aggregate_acceleration", "accelerated_segments")
                - metric(before, "aggregate_acceleration", "accelerated_segments"),
            (2 * FIXTURE_ROWS) as u64
        ),
        AnalyticalFamily::Graph => {
            assert!(metric(after, "graph", "last_reads") <= FIXTURE_ROWS as u64);
            assert!(metric(after, "graph", "last_candidates") <= FIXTURE_ROWS as u64);
        }
    }
}

#[test]
fn should_reject_each_analytical_path_atomically_given_the_same_low_memory_budget() {
    let _hook_guard = query_scan_control_test_guard();
    for family in AnalyticalFamily::ALL {
        // Arrange
        let fixture = Fixture::new(family, LOW_MEMORY_BYTES);
        let before = fixture.cassie.metrics();
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let error = fixture
            .cassie
            .execute_sql(&fixture.session, family.query(), vec![])
            .expect_err("low-budget analytical query should be atomic");
        let after = fixture.cassie.metrics();
        let reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);

        // Assert
        assert!(
            matches!(error, CassieError::ResourceLimit(_)),
            "{} should report SQLSTATE 54000, got {error:?}",
            family.label()
        );
        assert_failed_path_metrics_unchanged(family, &before, &after);
        assert!(
            reads <= family.controlled_read_bound(),
            "{} low-memory read bound: {reads}",
            family.label()
        );
        assert_query_cleanup(&fixture.cassie);
        fixture.cleanup();
        assert_eq!(
            wire_sqlstate(family, LOW_MEMORY_BYTES, None),
            "54000",
            "{} pgwire low-memory SQLSTATE",
            family.label()
        );
    }
}

#[test]
fn should_cancel_each_analytical_path_after_three_controlled_reads_without_partial_metrics() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let fixtures =
        AnalyticalFamily::ALL.map(|family| (family, Fixture::new(family, NORMAL_MEMORY_BUDGET)));

    // Act
    for (family, fixture) in fixtures {
        let before_metrics = fixture.cassie.metrics();
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(3));
        let error = fixture
            .cassie
            .execute_sql(&fixture.session, family.query(), vec![])
            .expect_err("controlled analytical read should cancel");
        set_query_scan_cancellation_after_entries(None);
        let after_metrics = fixture.cassie.metrics();
        let reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);

        // Assert
        assert!(
            matches!(error, CassieError::QueryCancelled),
            "{} should report SQLSTATE 57014, got {error:?}",
            family.label()
        );
        assert_eq!(reads, 3, "{} cancellation boundary", family.label());
        assert_failed_path_metrics_unchanged(family, &before_metrics, &after_metrics);
        assert_query_cleanup(&fixture.cassie);
        fixture.cleanup();
        assert_eq!(
            wire_sqlstate(family, NORMAL_MEMORY_BUDGET, Some(3)),
            "57014",
            "{} pgwire cancellation SQLSTATE",
            family.label()
        );
    }
}

#[test]
fn should_publish_only_deterministic_bounded_final_analytical_paths() {
    let _hook_guard = query_scan_control_test_guard();
    for family in AnalyticalFamily::ALL {
        // Arrange
        let fixture = Fixture::new(family, NORMAL_MEMORY_BUDGET);
        let before_metrics = fixture.cassie.metrics();
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let first = fixture
            .cassie
            .execute_sql(&fixture.session, family.query(), vec![])
            .expect("first analytical query");
        let second = fixture
            .cassie
            .execute_sql(&fixture.session, family.query(), vec![])
            .expect("second analytical query");
        let selected_metrics = fixture.cassie.metrics();
        let selected_reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);
        let fallback = exact_fallback_evidence(family, &fixture);
        let final_metrics = &fallback.final_metrics;

        // Assert
        assert_eq!(first.rows, second.rows, "{} ordering", family.label());
        if matches!(family, AnalyticalFamily::Graph) {
            assert_ne!(fallback.rows, first.rows, "graph overlay visibility");
            assert!(
                fallback.rows.iter().any(|row| {
                    row.first() == Some(&cassie::types::Value::String("edge-extra".to_string()))
                }),
                "graph overlay row visibility"
            );
        } else {
            assert_eq!(
                fallback.rows,
                first.rows,
                "{} exact fallback",
                family.label()
            );
        }
        if matches!(family, AnalyticalFamily::ColumnAggregate) {
            assert_eq!(
                first.rows,
                vec![vec![
                    cassie::types::Value::Int64(64),
                    cassie::types::Value::Int64(2016),
                    cassie::types::Value::Float64(31.5),
                ]]
            );
        } else {
            assert_eq!(first.rows.len(), RESULT_LIMIT);
        }
        assert_success_metrics(family, &before_metrics, &selected_metrics);
        assert!(
            selected_reads <= 2 * family.controlled_read_bound(),
            "{} controlled read bound: {selected_reads}",
            family.label()
        );
        if matches!(family, AnalyticalFamily::Graph) {
            let overlay_metrics = fallback
                .overlay_metrics
                .as_ref()
                .expect("graph overlay metrics");
            assert_eq!(
                metric(overlay_metrics, "graph", "traversals")
                    - metric(&selected_metrics, "graph", "traversals"),
                1
            );
            assert_eq!(
                overlay_metrics["graph"]["last_fallback_reason"].as_str(),
                Some("transaction-overlay")
            );
            assert!(metric(overlay_metrics, "graph", "last_reads") <= 65);
            assert!(metric(overlay_metrics, "graph", "last_candidates") <= 65);
            assert!(
                metric(overlay_metrics, "graph", "reads")
                    - metric(&selected_metrics, "graph", "reads")
                    <= 65
            );
            assert!(
                metric(overlay_metrics, "graph", "candidates")
                    - metric(&selected_metrics, "graph", "candidates")
                    <= 65
            );
            assert_eq!(
                metric(final_metrics, "graph", "traversals")
                    - metric(overlay_metrics, "graph", "traversals"),
                1
            );
            assert!(metric(final_metrics, "graph", "last_reads") <= FIXTURE_ROWS as u64);
            assert!(metric(final_metrics, "graph", "last_candidates") <= FIXTURE_ROWS as u64);
        } else {
            assert_eq!(
                family.successful_paths(final_metrics),
                family.successful_paths(&selected_metrics),
                "{} fallback published accelerator success",
                family.label()
            );
        }
        assert_query_cleanup(&fixture.cassie);
        fixture.cleanup();
    }
}
