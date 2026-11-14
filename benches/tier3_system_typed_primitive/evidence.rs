//! Untimed private-event observations and public diagnostics. No public typed API.
use super::fixture::{Case, Db};
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Value>>>);
struct Fields(Map<String, Value>);
impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().into(), json!(format!("{value:?}")));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().into(), json!(value));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), json!(value));
    }
}
impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        if event.metadata().target() == "cassie::typed_profile" {
            let mut fields = Fields(Map::new());
            event.record(&mut fields);
            self.0
                .lock()
                .expect("profile events")
                .push(Value::Object(fields.0));
        }
    }
}
fn snapshots(db: &Db) -> Value {
    let native = db.cassie.midge.native_memory_diagnostics().map_or_else(
        |error| json!({"error":error.to_string()}),
        |snapshot| json!(snapshot),
    );
    json!({
        "metrics":db.cassie.metrics(),
        "source_marker_reads":db.cassie.midge.query_scan_entries_for_diagnostics(),
        "column_validation_reads":db.cassie.midge.column_validation_reads_for_diagnostics(),
        "native":native,
    })
}
fn delta(before: &Value, after: &Value, path: &str) -> u64 {
    let before = before
        .pointer(path)
        .and_then(Value::as_u64)
        .expect("existing diagnostic counter");
    let after = after
        .pointer(path)
        .and_then(Value::as_u64)
        .expect("existing diagnostic counter");
    after
        .checked_sub(before)
        .expect("monotonic diagnostic counter")
}
fn deltas(before: &Value, after: &Value) -> Value {
    let mut values = Map::new();
    for path in [
        "/source_marker_reads",
        "/column_validation_reads",
        "/metrics/column_batches/scans",
        "/metrics/column_batches/materialized_values",
        "/metrics/column_batches/chunks_read",
        "/metrics/column_batches/segments_read",
        "/metrics/column_batches/physical_bytes_total",
        "/metrics/column_batches/logical_bytes_total",
        "/metrics/column_batches/selected_rows",
        "/metrics/column_batches/fallback_scans",
        "/metrics/column_batches/row_blob_fetches",
        "/metrics/aggregate_acceleration/scans",
        "/metrics/aggregate_acceleration/direct_scans",
        "/metrics/aggregate_acceleration/direct_materialized_values",
        "/metrics/execution_result_cache/hits",
    ] {
        values.insert(path.into(), json!(delta(before, after, path)));
    }
    Value::Object(values)
}
pub fn preflight(db: &Db, case: Case) -> (String, String, String) {
    let result = db.query(&format!("EXPLAIN {}", case.sql()));
    let Some(cassie::types::Value::String(text)) =
        result.rows().first().and_then(|row| row.first())
    else {
        panic!("textual EXPLAIN");
    };
    let token = |name: &str| {
        text.split_ascii_whitespace()
            .find_map(|token| {
                let (key, value) = token.split_once('=')?;
                (key == name).then(|| {
                    value
                        .trim_matches(|c: char| matches!(c, ',' | '|' | '[' | ']'))
                        .to_owned()
                })
            })
            .unwrap_or_else(|| panic!("EXPLAIN lacks {name}: {text}"))
    };
    (token("access_path"), token("fallback_reason"), text.clone())
}
fn source_metadata(db: &Db, case: Case) -> Value {
    let collection = db
        .cassie
        .catalog
        .get_collection(case.table)
        .expect("source collection");
    let mode = db
        .cassie
        .catalog
        .collection_storage_mode(&collection.name)
        .expect("actual storage mode");
    let expected = if case.authority == "column" {
        "column-store"
    } else if case.derived_index {
        "column-indexed"
    } else {
        "row-store"
    };
    assert_eq!(mode.as_str(), expected);
    let indexes = db.cassie.catalog.list_indexes(&collection.name);
    let covering = indexes
        .iter()
        .filter(|index| {
            index.kind == cassie::catalog::IndexKind::Column
                && ["n", "b"]
                    .iter()
                    .all(|field| index.fields.iter().any(|name| name == field))
        })
        .count();
    assert_eq!(covering, usize::from(case.derived_index));
    json!({"canonical_collection":collection.name,"storage_mode":mode.as_str(),"storage_version":collection.storage_version,"indexes":indexes})
}
pub fn observe(db: &Db, case: Case, explain: &str) -> Value {
    let source_metadata = source_metadata(db, case);
    let before = snapshots(db);
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let result = tracing::subscriber::with_default(subscriber, || {
        db.cassie.execute_sql(&db.session, &case.sql(), vec![])
    });
    let (query_success, exact_result, exact_descriptors, actual_rows, descriptors, error) =
        match result {
            Ok(result) => {
                let exact = result.rows() == case.expected();
                let descriptors = result.columns().iter().map(|column| json!({"oid":column.type_oid,"name":column.name,"data_type":column.data_type})).collect::<Vec<_>>();
                let exact_descriptors = result
                    .columns()
                    .iter()
                    .map(|column| column.type_oid)
                    .collect::<Vec<_>>()
                    == [1700, 20, 20];
                (
                    true,
                    exact,
                    exact_descriptors,
                    format!("{:?}", result.rows()),
                    descriptors,
                    None,
                )
            }
            Err(error) => (
                false,
                false,
                false,
                String::new(),
                Vec::new(),
                Some(error.to_string()),
            ),
        };
    let after = snapshots(db);
    let delta = deltas(&before, &after);
    let events = capture.0.lock().expect("profile events").clone();
    let typed_completions = events
        .iter()
        .filter(|event| event["event"] == "typed_aggregate_complete")
        .count();
    let batches = events
        .iter()
        .filter(|event| event["event"] == "typed_transport_batch")
        .count();
    let path = if typed_completions > 0 && batches > 0 {
        "typed_aggregate_with_observed_transport"
    } else if delta["/metrics/aggregate_acceleration/scans"]
        .as_u64()
        .expect("counter")
        > 0
    {
        "accelerated_aggregate_without_typed_completion"
    } else {
        "completed_path_without_private_typed_transport_proof"
    };
    json!({
        "workload":case.name,"sql":case.sql(),"explain":explain,
        "authority":case.authority,"derived_index":case.derived_index,"source_metadata":source_metadata,
        "query_success":query_success,"exact_independent_result":exact_result,"exact_descriptors":exact_descriptors,"actual_rows":actual_rows,"error":error,
        "expected_rows":format!("{:?}",case.expected()),"descriptors":descriptors,
        "path_classification":path,"typed_operator_completions":typed_completions,"completed_transport_batches":batches,
        "events":events,"event_scope":"completed_batches_are_attempt_scoped_until_whole_sql_success; aggregate event is operator completion",
        "conversion_bytes_unit":"inline Value carrier slots only; not allocator traffic or RSS",
        "admission_scope":"private peak at typed operator completion; public query peak is cumulative runtime high water",
        "before":before,"after":after,"counter_deltas":delta,
        "allocation_counts":"unavailable_without_external_profiler","allocation_bytes":"unavailable_without_external_profiler",
        "timed_collection":false,
    })
}
pub fn assert_valid(receipt: &Value) {
    assert_eq!(
        receipt["query_success"], true,
        "diagnostic SQL must succeed: {receipt}"
    );
    assert_eq!(
        receipt["exact_independent_result"], true,
        "independent result: {receipt}"
    );
    assert_eq!(
        receipt["exact_descriptors"], true,
        "SUM NUMERIC/COUNT BIGINT metadata"
    );
    assert_eq!(
        receipt["counter_deltas"]["/metrics/execution_result_cache/hits"], 0,
        "cache must be off"
    );
    assert_eq!(
        receipt["after"]["metrics"]["query"]["current_accounted_memory_bytes"], 0,
        "released diagnostic query owners"
    );
}
pub fn write(mode: &str, records: &[Value]) {
    let path = std::env::var("BENCH_TYPED_PRIMITIVE_EVIDENCE_OUT")
        .unwrap_or_else(|_| format!("target/assembly-line/typed-primitive-profile/{mode}.json"));
    let path = std::path::Path::new(&path);
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).expect("artifact directory");
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(records).expect("measurement receipt"),
    )
    .expect("write measurement receipt");
}
