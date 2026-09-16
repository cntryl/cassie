use crate::support_sql::{time_series_sidecar_records, use_local_storage};
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;
use serde_json::Value as JsonValue;
use std::path::PathBuf;
use uuid::Uuid;

const TABLE: &str = "time_series_width_evidence";
const INDEX: &str = "time_series_width_evidence_idx";

pub struct TimeSeriesWidthEvidence {
    cases: Vec<WidthObservation>,
}

struct WidthCase {
    slug: &'static str,
    width: &'static str,
    lower: &'static str,
    upper: &'static str,
}

struct WidthObservation {
    width: &'static str,
    indexed: ScenarioResults,
    baseline: ScenarioResults,
    metrics: JsonValue,
    plan: String,
    sidecars_before_retention: usize,
    rows_before_retention: usize,
    sidecars_after_retention: usize,
    rows_after_retention: usize,
}

#[derive(Debug, PartialEq)]
struct ScenarioResults {
    bounded: Vec<Vec<Value>>,
    negative_epoch: Vec<Vec<Value>>,
    empty_bucket: Vec<Vec<Value>>,
    paged: Vec<Vec<Value>>,
    after_mutations: Vec<Vec<Value>>,
    after_retention: Vec<Vec<Value>>,
}

impl TimeSeriesWidthEvidence {
    pub fn collect() -> Self {
        use_local_storage();
        let cases = width_cases()
            .into_iter()
            .map(|case| observe_width(&case))
            .collect::<Vec<_>>();
        Self { cases }
    }

    pub fn widths(&self) -> Vec<&'static str> {
        self.cases.iter().map(|case| case.width).collect()
    }

    pub fn assert_exact_equivalence(&self) {
        for case in &self.cases {
            assert_eq!(case.indexed, case.baseline, "width={}", case.width);
            assert_eq!(
                case.sidecars_before_retention, case.rows_before_retention,
                "width={} sidecars before retention",
                case.width
            );
            assert_eq!(
                case.sidecars_after_retention, case.rows_after_retention,
                "width={} sidecars after retention",
                case.width
            );
            assert!(
                case.plan
                    .contains(&format!("time_series=bucket_width:{}", case.width)),
                "plan={}",
                case.plan
            );
            assert!(
                case.plan.contains("time_series_storage=bucket-native-v1"),
                "plan={}",
                case.plan
            );
            assert!(
                case.metrics["time_series"]["bucket_native_hits"]
                    .as_u64()
                    .unwrap_or_default()
                    >= 4,
                "metrics={}",
                case.metrics
            );
            assert_eq!(case.metrics["time_series"]["fallback_scans"], 0);
            assert!(
                case.metrics["time_series"]["buckets_scanned"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0
            );
            assert!(
                case.metrics["time_series"]["buckets_skipped"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0
            );
            assert!(
                case.metrics["time_series"]["index_entries_scanned"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0
            );
            assert!(
                case.metrics["time_series"]["row_point_fetches"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0
            );
            assert_eq!(case.metrics["query"]["current_accounted_memory_bytes"], 0);
            assert_eq!(case.metrics["runtime"]["active_operator_workers"], 0);
        }
    }
}

fn width_cases() -> [WidthCase; 3] {
    [
        WidthCase {
            slug: "minutes_15",
            width: "15 minutes",
            lower: "1969-12-31T23:45:00Z",
            upper: "1970-01-01T00:15:00Z",
        },
        WidthCase {
            slug: "hour_1",
            width: "1 hour",
            lower: "1969-12-31T23:00:00Z",
            upper: "1970-01-01T01:00:00Z",
        },
        WidthCase {
            slug: "day_1",
            width: "1 day",
            lower: "1969-12-31T00:00:00Z",
            upper: "1970-01-02T00:00:00Z",
        },
    ]
}

fn observe_width(case: &WidthCase) -> WidthObservation {
    let indexed_path = evidence_path(case.slug, "indexed");
    let baseline_path = evidence_path(case.slug, "baseline");
    let indexed = Cassie::new_with_data_dir(&indexed_path).expect("indexed Cassie");
    let baseline = Cassie::new_with_data_dir(&baseline_path).expect("baseline Cassie");
    indexed.startup().expect("start indexed Cassie");
    baseline.startup().expect("start baseline Cassie");
    let indexed_session = indexed.create_session("time-series-evidence", None);
    let baseline_session = baseline.create_session("time-series-evidence", None);
    seed_fixture(&indexed, &indexed_session, Some(case.width));
    seed_fixture(&baseline, &baseline_session, None);
    let plan = explain(&indexed, &indexed_session, case.lower, case.upper);

    let mut indexed_results = run_read_scenarios(&indexed, &indexed_session, case);
    let mut baseline_results = run_read_scenarios(&baseline, &baseline_session, case);
    apply_mutations(&indexed, &indexed_session);
    apply_mutations(&baseline, &baseline_session);
    indexed_results.after_mutations = all_rows(&indexed, &indexed_session);
    baseline_results.after_mutations = all_rows(&baseline, &baseline_session);
    let sidecars_before_retention = time_series_sidecar_records(&indexed, TABLE, INDEX).len();
    let rows_before_retention = indexed_results.after_mutations.len();
    apply_retention(&indexed, &indexed_session);
    apply_retention(&baseline, &baseline_session);
    indexed_results.after_retention = all_rows(&indexed, &indexed_session);
    baseline_results.after_retention = all_rows(&baseline, &baseline_session);
    let sidecars_after_retention = time_series_sidecar_records(&indexed, TABLE, INDEX).len();
    let rows_after_retention = indexed_results.after_retention.len();
    let metrics = indexed.metrics();

    drop(indexed_session);
    drop(baseline_session);
    drop(indexed);
    drop(baseline);
    let _ = std::fs::remove_dir_all(indexed_path);
    let _ = std::fs::remove_dir_all(baseline_path);

    WidthObservation {
        width: case.width,
        indexed: indexed_results,
        baseline: baseline_results,
        metrics,
        plan,
        sidecars_before_retention,
        rows_before_retention,
        sidecars_after_retention,
        rows_after_retention,
    }
}

fn seed_fixture(cassie: &Cassie, session: &CassieSession, width: Option<&str>) {
    cassie
        .execute_sql(
            session,
            &format!(
                "CREATE TABLE {TABLE} (event_id TEXT, tenant TEXT, event_at TIMESTAMP, amount BIGINT)"
            ),
            vec![],
        )
        .expect("create time-series evidence table");
    cassie
        .execute_sql(
            session,
            &format!(
                "INSERT INTO {TABLE} (event_id, tenant, event_at, amount) VALUES {}",
                fixture_values()
            ),
            vec![],
        )
        .expect("seed time-series evidence rows");
    if let Some(width) = width {
        cassie
            .execute_sql(
                session,
                &format!(
                    "CREATE INDEX {INDEX} ON {TABLE} USING time_series (event_at) WITH (bucket_width = '{width}', partition_by = tenant)"
                ),
                vec![],
            )
            .expect("create time-series evidence index");
    }
}

fn fixture_values() -> String {
    [
        ("e00", "acme", "1969-12-30T23:59:59Z", 0),
        ("e01", "acme", "1969-12-31T00:00:00Z", 1),
        ("e02", "globex", "1969-12-31T22:59:59Z", 2),
        ("e03", "acme", "1969-12-31T23:00:00Z", 3),
        ("e04", "acme", "1969-12-31T23:44:59Z", 4),
        ("e05", "acme", "1969-12-31T23:45:00Z", 5),
        ("e06", "globex", "1969-12-31T23:59:59Z", 6),
        ("e07", "acme", "1970-01-01T00:00:00Z", 7),
        ("e08", "acme", "1970-01-01T00:14:59Z", 8),
        ("e09", "acme", "1970-01-01T00:15:00Z", 9),
        ("e10", "globex", "1970-01-01T00:59:59Z", 10),
        ("e11", "acme", "1970-01-01T01:00:00Z", 11),
        ("e12", "acme", "1970-01-01T23:59:59Z", 12),
        ("e13", "acme", "1970-01-02T00:00:00Z", 13),
        ("e14", "globex", "1970-01-03T00:00:00Z", 14),
    ]
    .into_iter()
    .map(|(id, tenant, timestamp, amount)| format!("('{id}', '{tenant}', '{timestamp}', {amount})"))
    .collect::<Vec<_>>()
    .join(", ")
}

fn run_read_scenarios(
    cassie: &Cassie,
    session: &CassieSession,
    case: &WidthCase,
) -> ScenarioResults {
    let bounded = cassie
        .execute_sql(
            session,
            &format!(
                "SELECT event_id, event_at, amount FROM {TABLE} WHERE tenant = $1 AND event_at >= $2 AND event_at < $3 ORDER BY event_at, event_id"
            ),
            vec![
                Value::String("acme".to_string()),
                Value::String(case.lower.to_string()),
                Value::String(case.upper.to_string()),
            ],
        )
        .expect("bounded time-series evidence query")
        .rows;
    let negative_epoch = query_rows(
        cassie,
        session,
        "SELECT event_id, event_at FROM time_series_width_evidence WHERE event_at >= '1969-12-31T23:44:59Z' AND event_at <= '1970-01-01T00:00:00Z' ORDER BY event_at, event_id",
    );
    let empty_bucket = query_rows(
        cassie,
        session,
        "SELECT event_id FROM time_series_width_evidence WHERE event_at >= '1971-01-01T00:00:00Z' AND event_at < '1971-01-02T00:00:00Z' ORDER BY event_at",
    );
    let paged = query_rows(
        cassie,
        session,
        "SELECT event_id, event_at FROM time_series_width_evidence WHERE event_at >= '1969-12-31T00:00:00Z' ORDER BY event_at, event_id LIMIT 4 OFFSET 3",
    );
    ScenarioResults {
        bounded,
        negative_epoch,
        empty_bucket,
        paged,
        after_mutations: Vec::new(),
        after_retention: Vec::new(),
    }
}

fn apply_mutations(cassie: &Cassie, session: &CassieSession) {
    for sql in [
        "INSERT INTO time_series_width_evidence (event_id, tenant, event_at, amount) VALUES ('mut', 'acme', '1970-01-01T02:00:00Z', 100)",
        "UPDATE time_series_width_evidence SET tenant = 'globex', event_at = '1970-01-02T12:00:00Z' WHERE event_id = 'mut'",
        "DELETE FROM time_series_width_evidence WHERE event_id = 'e14'",
    ] {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("apply time-series evidence mutation");
    }
}

fn apply_retention(cassie: &Cassie, session: &CassieSession) {
    for sql in [
        "CREATE RETENTION POLICY time_series_width_retention ON time_series_width_evidence USING event_at RETAIN FOR '1 day'",
        "ENFORCE RETENTION POLICY time_series_width_retention AT '1970-01-03T00:00:00Z'",
    ] {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("apply time-series evidence retention");
    }
}

fn all_rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    query_rows(
        cassie,
        session,
        "SELECT event_id, tenant, event_at, amount FROM time_series_width_evidence ORDER BY event_at, event_id",
    )
}

fn query_rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("execute time-series evidence query")
        .rows
}

fn explain(cassie: &Cassie, session: &CassieSession, lower: &str, upper: &str) -> String {
    cassie
        .execute_sql(
            session,
            &format!(
                "EXPLAIN SELECT event_id FROM {TABLE} WHERE tenant = 'acme' AND event_at >= '{lower}' AND event_at < '{upper}'"
            ),
            vec![],
        )
        .expect("explain time-series evidence query")
        .rows[0][0]
        .as_str()
        .expect("textual time-series evidence plan")
        .to_string()
}

fn evidence_path(slug: &str, mode: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cassie-time-series-evidence-{slug}-{mode}-{}",
        Uuid::new_v4()
    ))
}
