use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use cassie::types::Value;
use serde_json::{json, Value as JsonValue};

#[path = "support/sql.rs"]
mod support;

const TABLE: &str = "ts_complete_events";
const INDEX: &str = "idx_ts_complete_time";
const QUERY: &str = "SELECT amount FROM ts_complete_events WHERE event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-01T02:00:00Z' ORDER BY amount";

fn fixture(label: &str) -> (Cassie, String) {
    support::with_fallback();
    let path = support::data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE ts_complete_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX idx_ts_complete_time ON ts_complete_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
            vec![],
        )
        .expect("create index");
    cassie
        .midge
        .put_fresh_time_series_documents(
            &canonical_collection(&cassie),
            vec![
                (
                    Some("event-1".to_string()),
                    json!({"tenant":"acme","event_at":"2026-01-01T00:00:00Z","amount":10}),
                ),
                (
                    Some("event-2".to_string()),
                    json!({"tenant":"acme","event_at":"2026-01-01T00:30:00Z","amount":20}),
                ),
                (
                    Some("event-3".to_string()),
                    json!({"tenant":"acme","event_at":"2026-01-01T01:00:00Z","amount":30}),
                ),
            ],
        )
        .expect("seed documents");
    (cassie, path)
}

fn canonical_collection(cassie: &Cassie) -> String {
    cassie
        .catalog
        .get_schema(TABLE)
        .expect("table metadata")
        .collection
}

fn canonical_index(cassie: &Cassie) -> String {
    cassie
        .catalog
        .get_index(&canonical_collection(cassie), INDEX)
        .expect("index metadata")
        .name
}

fn membership_entries(cassie: &Cassie) -> Vec<(Vec<u8>, Vec<u8>)> {
    let prefix = cassie
        .midge
        .time_series_index_prefix_for_diagnostics(
            &canonical_collection(cassie),
            &canonical_index(cassie),
        )
        .expect("membership prefix");
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("membership entries")
}

fn manifest_key(cassie: &Cassie) -> Vec<u8> {
    cassie
        .midge
        .time_series_manifest_key_for_diagnostics(
            &canonical_collection(cassie),
            &canonical_index(cassie),
        )
        .expect("manifest key")
}

fn manifest(cassie: &Cassie) -> JsonValue {
    serde_json::from_slice(
        &cassie
            .midge
            .raw_get(StorageFamily::Data, &manifest_key(cassie))
            .expect("read manifest")
            .expect("manifest exists"),
    )
    .expect("valid manifest")
}

fn bucket_count_entries(cassie: &Cassie) -> Vec<(Vec<u8>, Vec<u8>)> {
    let prefix = cassie
        .midge
        .time_series_bucket_count_prefix_for_diagnostics(
            &canonical_collection(cassie),
            &canonical_index(cassie),
        )
        .expect("bucket count prefix");
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("bucket count entries")
}

fn execute_query(cassie: &Cassie) -> cassie::executor::QueryResult {
    cassie
        .execute_sql(&cassie.create_session("tester", None), QUERY, vec![])
        .expect("time-series query")
}

fn expected_rows() -> Vec<Vec<Value>> {
    vec![
        vec![Value::Int64(10)],
        vec![Value::Int64(20)],
        vec![Value::Int64(30)],
    ]
}

fn assert_fallback(cassie: &Cassie, reason: &str) {
    let metrics = cassie.metrics();
    assert_eq!(metrics["time_series"]["fallback_scans"].as_u64(), Some(1));
    assert_eq!(
        metrics["time_series"]["last_fallback_reason"].as_str(),
        Some(reason)
    );
}

#[test]
fn should_fallback_given_one_missing_membership_among_valid_bucket_entries() {
    // Arrange
    let (cassie, path) = fixture("time-series-one-missing-membership");
    let missing = membership_entries(&cassie)
        .into_iter()
        .find(|(key, _)| key.windows(b"event-2".len()).any(|part| part == b"event-2"))
        .expect("event-2 membership");
    cassie
        .midge
        .raw_delete(StorageFamily::Data, &missing.0)
        .expect("delete one membership");

    // Act
    let result = execute_query(&cassie);

    // Assert
    assert_eq!(result.rows, expected_rows());
    assert_fallback(&cassie, "incomplete-bucket-membership");
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_given_dangling_membership_with_surviving_valid_entries() {
    // Arrange
    let (cassie, path) = fixture("time-series-dangling-membership");
    let (key, value) = membership_entries(&cassie)
        .into_iter()
        .find(|(key, _)| key.windows(b"event-2".len()).any(|part| part == b"event-2"))
        .expect("event-2 membership");
    let mut dangling_key = key.clone();
    let offset = dangling_key
        .windows(b"event-2".len())
        .position(|part| part == b"event-2")
        .expect("membership id offset");
    dangling_key[offset..offset + b"ghost-2".len()].copy_from_slice(b"ghost-2");
    cassie
        .midge
        .raw_delete(StorageFamily::Data, &key)
        .expect("delete source membership");
    cassie
        .midge
        .raw_put(StorageFamily::Data, &dangling_key, &value)
        .expect("write dangling membership");

    // Act
    let result = execute_query(&cassie);

    // Assert
    assert_eq!(result.rows, expected_rows());
    assert_fallback(&cassie, "dangling-bucket-membership");
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_given_missing_bucket_count_metadata() {
    // Arrange
    let (cassie, path) = fixture("time-series-missing-bucket-count");
    let (key, _) = bucket_count_entries(&cassie)
        .into_iter()
        .next()
        .expect("bucket count");
    cassie
        .midge
        .raw_delete(StorageFamily::Data, &key)
        .expect("delete bucket count");

    // Act
    let result = execute_query(&cassie);

    // Assert
    assert_eq!(result.rows, expected_rows());
    assert_fallback(&cassie, "missing-bucket-metadata");
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_given_corrupt_bucket_count() {
    // Arrange
    let (cassie, path) = fixture("time-series-corrupt-bucket-count");
    let (key, _) = bucket_count_entries(&cassie)
        .into_iter()
        .next()
        .expect("bucket count");
    cassie
        .midge
        .raw_put(StorageFamily::Data, &key, b"corrupt")
        .expect("corrupt bucket count");

    // Act
    let result = execute_query(&cassie);

    // Assert
    assert_eq!(result.rows, expected_rows());
    assert_fallback(&cassie, "corrupt-bucket-metadata");
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_given_corrupt_manifest_total() {
    // Arrange
    let (cassie, path) = fixture("time-series-corrupt-total");
    let mut corrupted = manifest(&cassie);
    corrupted["total_membership"] = json!(99);
    cassie
        .midge
        .raw_put(
            StorageFamily::Data,
            &manifest_key(&cassie),
            &serde_json::to_vec(&corrupted).expect("encode corrupt manifest"),
        )
        .expect("corrupt manifest total");

    // Act
    let result = execute_query(&cassie);

    // Assert
    assert_eq!(result.rows, expected_rows());
    assert_fallback(&cassie, "corrupt-bucket-metadata");
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_given_stale_manifest_generation() {
    // Arrange
    let (cassie, path) = fixture("time-series-stale-generation");
    let mut corrupted = manifest(&cassie);
    corrupted["generation"] = json!(0);
    cassie
        .midge
        .raw_put(
            StorageFamily::Data,
            &manifest_key(&cassie),
            &serde_json::to_vec(&corrupted).expect("encode stale manifest"),
        )
        .expect("stale manifest generation");

    // Act
    let result = execute_query(&cassie);

    // Assert
    assert_eq!(result.rows, expected_rows());
    assert_fallback(&cassie, "stale-bucket-metadata");
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rebuild_missing_manifest_during_restart() {
    // Arrange
    let (cassie, path) = fixture("time-series-restart-missing-manifest");
    cassie
        .midge
        .raw_delete(StorageFamily::Data, &manifest_key(&cassie))
        .expect("delete manifest");
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
    restarted.startup().expect("restart reconciliation");
    let result = execute_query(&restarted);

    // Assert
    assert_eq!(result.rows, expected_rows());
    assert_eq!(manifest(&restarted)["version"].as_u64(), Some(1));
    assert_eq!(
        restarted.metrics()["time_series"]["bucket_native_hits"].as_u64(),
        Some(1)
    );
    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rebuild_old_manifest_version_during_restart() {
    // Arrange
    let (cassie, path) = fixture("time-series-restart-old-manifest");
    let mut old = manifest(&cassie);
    old["version"] = json!(0);
    cassie
        .midge
        .raw_put(
            StorageFamily::Data,
            &manifest_key(&cassie),
            &serde_json::to_vec(&old).expect("encode old manifest"),
        )
        .expect("write old manifest");
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
    restarted.startup().expect("restart reconciliation");
    let result = execute_query(&restarted);

    // Assert
    assert_eq!(result.rows, expected_rows());
    assert_eq!(manifest(&restarted)["version"].as_u64(), Some(1));
    assert_eq!(
        restarted.metrics()["time_series"]["bucket_native_hits"].as_u64(),
        Some(1)
    );
    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_maintain_exact_metadata_after_bulk_update_and_delete() {
    // Arrange
    let (cassie, path) = fixture("time-series-lifecycle-metadata");
    let session = cassie.create_session("tester", None);

    // Act
    cassie
        .execute_sql(
            &session,
            "UPDATE ts_complete_events SET event_at = '2026-01-01T01:30:00Z' WHERE amount = 20",
            vec![],
        )
        .expect("update bucket membership");
    cassie
        .execute_sql(
            &session,
            "DELETE FROM ts_complete_events WHERE amount = 30",
            vec![],
        )
        .expect("delete bucket membership");
    let metadata = manifest(&cassie);
    let counts = bucket_count_entries(&cassie);

    // Assert
    assert_eq!(metadata["version"].as_u64(), Some(1));
    assert_eq!(metadata["total_membership"].as_u64(), Some(2));
    assert_eq!(
        metadata["generation"].as_u64(),
        Some(
            cassie
                .midge
                .collection_generation(&canonical_collection(&cassie))
                .expect("collection generation")
        )
    );
    assert_eq!(counts.len(), 2);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
