//! Private profiling observations follow actual completed transport, not plan labels.
use super::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

type Record = BTreeMap<String, String>;
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Record>>>);
struct Fields(Record);
impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().into(), value.to_string());
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
}
impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        if event.metadata().target() == "cassie::typed_profile" {
            let mut values = Fields(BTreeMap::new());
            event.record(&mut values);
            self.0.lock().expect("events").push(values.0);
        }
    }
}
impl Capture {
    fn run<T>(&self, work: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::registry().with(self.clone());
        tracing::subscriber::with_default(subscriber, work)
    }
    fn records(&self) -> Vec<Record> {
        self.0.lock().expect("events").clone()
    }
}
fn fixture(encoded: bool) -> (Cassie, CassieSession) {
    fixture_with_result_limit(encoded, None)
}
fn fixture_with_result_limit(
    encoded: bool,
    result_limit: Option<usize>,
) -> (Cassie, CassieSession) {
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled =
        crate::config::ExecutionResultCacheEnabled::disabled();
    if let Some(limit) = result_limit {
        config.limits.max_result_rows = limit;
    }
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE profile_lanes (n BIGINT, b BOOLEAN)",
        "INSERT INTO profile_lanes VALUES (9007199254740993,TRUE),(NULL,TRUE),(7,NULL),(-2,FALSE)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("fixture");
    }
    if encoded {
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX profile_cover ON profile_lanes USING column (n,b)",
                vec![],
            )
            .expect("covering index");
    }
    (cassie, session)
}
fn number(record: &Record, key: &str) -> usize {
    record[key].parse().expect("unsigned observation")
}

#[test]
fn should_observe_document_carriers_at_the_completed_batch_boundary() {
    // Arrange
    let (cassie, session) = fixture(false);
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let capture = Capture::default();

    // Act
    let batch = capture.run(|| {
        let mut stream = TypedScanStream::open(
            &cassie,
            Some(&session),
            "profile_lanes",
            &["n".into(), "b".into()],
            &controls,
        )
        .expect("open")
        .expect("typed");
        stream.next_batch().expect("batch").expect("rows")
    });
    let records = capture.records();

    // Assert
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["source"], "documents");
    assert_eq!(number(&records[0], "rows"), 4);
    assert_eq!(number(&records[0], "document_field_lanes"), 8);
    assert_eq!(number(&records[0], "value_carrier_slots"), 8);
    assert_eq!(
        number(&records[0], "value_carrier_bytes"),
        8 * size_of::<Value>()
    );
    drop(batch);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_distinguish_numeric_encoded_lanes_from_json_backed_boolean_lanes() {
    // Arrange
    let (cassie, session) = fixture(true);
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let capture = Capture::default();

    // Act
    capture.run(|| {
        let mut stream = TypedScanStream::open(
            &cassie,
            Some(&session),
            "profile_lanes",
            &["n".into(), "b".into()],
            &controls,
        )
        .expect("open")
        .expect("typed");
        assert!(stream.is_encoded());
        let batch = stream.next_batch().expect("batch").expect("rows");
        assert_eq!(batch.len(), 4);
    });
    let records = capture.records();

    // Assert
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["source"], "encoded");
    assert_eq!(number(&records[0], "encoded_numeric_lanes"), 4);
    assert_eq!(number(&records[0], "encoded_json_lanes"), 4);
    assert_eq!(number(&records[0], "value_carrier_slots"), 4);
    assert_eq!(
        number(&records[0], "value_carrier_bytes"),
        4 * size_of::<Value>()
    );
    assert_eq!(number(&records[0], "document_field_lanes"), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_withhold_completion_observations_after_cancellation() {
    // Arrange
    let (cassie, session) = fixture(false);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
        cancellation.clone(),
    );
    let mut stream = TypedScanStream::open(
        &cassie,
        Some(&session),
        "profile_lanes",
        &["n".into()],
        &controls,
    )
    .expect("open")
    .expect("typed");
    cancellation.cancel();
    let capture = Capture::default();

    // Act
    let result = capture.run(|| stream.next_batch());
    drop(stream);

    // Assert
    assert!(result.is_err());
    assert_eq!(capture.records(), Vec::<Record>::new());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_exact_sql_results_when_observation_collection_is_disabled() {
    // Arrange
    let (cassie, session) = fixture(false);
    let sql = "SELECT SUM(n),COUNT(n),COUNT(*) FROM profile_lanes WHERE b IS NOT FALSE AND n>=0";
    let capture = Capture::default();
    let before = cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let observed = capture.run(|| cassie.execute_sql(&session, sql, vec![]).expect("observed"));
    let observed_reads = cassie.midge.query_scan_entries_for_diagnostics() - before;
    let plain_before = cassie.midge.query_scan_entries_for_diagnostics();
    let plain =
        tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
            cassie.execute_sql(&session, sql, vec![]).expect("plain")
        });

    // Assert
    assert_eq!(
        observed.rows(),
        &[vec![
            Value::Int64(9_007_199_254_741_000),
            Value::Int64(2),
            Value::Int64(2)
        ]]
    );
    assert_eq!(plain.rows(), observed.rows());
    assert_eq!(
        cassie.midge.query_scan_entries_for_diagnostics() - plain_before,
        observed_reads
    );
    assert_eq!(
        capture
            .records()
            .iter()
            .filter(|record| record
                .get("event")
                .is_some_and(|value| value == "typed_aggregate_complete"))
            .count(),
        1
    );
    drop(observed);
    drop(plain);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_not_label_a_declined_grouped_path_as_typed_aggregate_completion() {
    // Arrange
    let (cassie, session) = fixture(false);
    let capture = Capture::default();

    // Act
    let result = capture.run(|| {
        cassie
            .execute_sql(
                &session,
                "SELECT b,COUNT(*) FROM profile_lanes GROUP BY b",
                vec![],
            )
            .expect("scalar grouped path")
    });

    // Assert
    assert_eq!(result.rows().len(), 3);
    assert!(capture.records().iter().all(|record| record
        .get("event")
        .is_none_or(|value| value != "typed_aggregate_complete")));
}

#[test]
fn should_withhold_transport_observations_after_admission_rejection() {
    // Arrange
    let (cassie, session) = fixture(false);
    let limits = crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: 0,
        ..crate::config::CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    let capture = Capture::default();

    // Act
    let result = capture.run(|| {
        TypedScanStream::open(
            &cassie,
            Some(&session),
            "profile_lanes",
            &["n".into()],
            &controls,
        )
    });

    // Assert
    assert!(result.is_err());
    assert_eq!(capture.records(), Vec::<Record>::new());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_completed_batch_events_attempt_scoped_when_result_admission_fails() {
    // Arrange
    let (cassie, session) = fixture_with_result_limit(false, Some(0));
    let capture = Capture::default();

    // Act
    let result = capture.run(|| {
        cassie.execute_sql(
            &session,
            "SELECT SUM(n) FROM profile_lanes WHERE b IS TRUE AND n>=0",
            vec![],
        )
    });
    let records = capture.records();

    // Assert
    assert!(result.is_err());
    assert!(records.iter().any(|record| record
        .get("event")
        .is_some_and(|value| value == "typed_transport_batch")));
    assert!(records.iter().all(|record| record
        .get("event")
        .is_none_or(|value| value != "typed_aggregate_complete")));
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}
