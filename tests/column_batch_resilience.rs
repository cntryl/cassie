use cassie::app::{Cassie, CassieSession};
use cassie::midge::adapter::{set_column_batch_maintenance_failure_point, StorageFamily};
use cassie::types::Value;
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::{canonical_test_collection, canonical_test_index, data_dir, with_fallback};

static COLUMN_BATCH_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct AmountFixture {
    test_guard: std::sync::MutexGuard<'static, ()>,
    path: String,
    cassie: Cassie,
    session: CassieSession,
    collection: String,
    index: String,
}

fn amount_fixture(label: &str, table: &str, values: &[Value]) -> AmountFixture {
    let test_guard = COLUMN_BATCH_FAILPOINT_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    with_fallback();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (amount BIGINT)"),
            vec![],
        )
        .expect("create table");
    for value in values {
        cassie
            .execute_sql(
                &session,
                &format!("INSERT INTO {table} (amount) VALUES ($1)"),
                vec![value.clone()],
            )
            .expect("insert amount");
    }
    let index_name = format!("{table}_column_idx");
    cassie
        .execute_sql(
            &session,
            &format!(
                "CREATE INDEX {index_name} ON {table} USING column (amount) WITH (segment_size = 1)"
            ),
            vec![],
        )
        .expect("create column index");
    let collection = canonical_test_collection(&cassie, table);
    let index = canonical_test_index(&cassie, &collection, &index_name);
    AmountFixture {
        test_guard,
        path,
        cassie,
        session,
        collection,
        index,
    }
}

fn ordered_numeric_fixture(
    label: &str,
    table: &str,
    column_type: &str,
    values: &[serde_json::Value],
    segment_size: usize,
) -> AmountFixture {
    let test_guard = COLUMN_BATCH_FAILPOINT_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    with_fallback();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (amount {column_type})"),
            vec![],
        )
        .expect("create table");
    let collection = canonical_test_collection(&cassie, table);
    for (position, value) in values.iter().enumerate() {
        cassie
            .midge
            .put_document(
                &collection,
                Some(format!("row-{position:04}")),
                serde_json::json!({ "amount": value }),
            )
            .expect("insert ordered amount");
    }
    let index_name = format!("{table}_column_idx");
    cassie
        .execute_sql(
            &session,
            &format!(
                "CREATE INDEX {index_name} ON {table} USING column (amount) WITH (segment_size = {segment_size})"
            ),
            vec![],
        )
        .expect("create column index");
    let index = canonical_test_index(&cassie, &collection, &index_name);
    AmountFixture {
        test_guard,
        path,
        cassie,
        session,
        collection,
        index,
    }
}

fn metadata_entry(fixture: &AmountFixture) -> (Vec<u8>, serde_json::Value) {
    fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan data")
        .into_iter()
        .find_map(|(key, raw)| {
            let metadata = serde_json::from_slice::<serde_json::Value>(&raw).ok()?;
            metadata
                .get("metadata_format_version")
                .is_some()
                .then_some((key, metadata))
        })
        .expect("column batch metadata")
}

fn write_metadata(fixture: &AmountFixture, key: Vec<u8>, metadata: &serde_json::Value) {
    let mut tx = fixture
        .cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("open data transaction");
    tx.put(
        key,
        serde_json::to_vec(metadata).expect("serialize metadata"),
        None,
    )
    .expect("write metadata");
    tx.commit(WriteOptions::sync())
        .expect("commit metadata mutation");
}

fn sum_amount(fixture: &AmountFixture, table: &str) -> Result<Vec<Vec<Value>>, String> {
    sum_amount_as(fixture, table, "total")
}

fn sum_amount_as(
    fixture: &AmountFixture,
    table: &str,
    alias: &str,
) -> Result<Vec<Vec<Value>>, String> {
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            &format!("SELECT SUM(amount) AS {alias} FROM {table}"),
            vec![],
        )
        .map(|result| result.rows)
        .map_err(|error| error.to_string())
}

fn restart_fixture(fixture: AmountFixture) -> AmountFixture {
    let AmountFixture {
        test_guard,
        path,
        cassie,
        session,
        collection,
        index,
    } = fixture;
    drop(session);
    drop(cassie);
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    cassie.startup().expect("reconcile column batches");
    let session = cassie.create_session("tester", None);
    AmountFixture {
        test_guard,
        path,
        cassie,
        session,
        collection,
        index,
    }
}

#[test]
fn should_preserve_integer_sum_above_two_to_the_fifty_third() {
    // Arrange
    let _test_guard = COLUMN_BATCH_FAILPOINT_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    with_fallback();
    let path = data_dir("column_batch_large_integer_sum");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE column_batch_large_integer_sum (amount BIGINT)",
            vec![],
        )
        .expect("create table");
    for amount in [9_007_199_254_740_993_i64, 2] {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_large_integer_sum (amount) VALUES ($1)",
                vec![Value::Int64(amount)],
            )
            .expect("insert amount");
    }
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX column_batch_large_integer_sum_idx ON column_batch_large_integer_sum USING column (amount) WITH (segment_size = 1)",
            vec![],
        )
        .expect("create column index");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT SUM(amount) AS total FROM column_batch_large_integer_sum",
            vec![],
        )
        .expect("aggregate large integers");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::Int64(9_007_199_254_740_995)]]);
    assert_eq!(cassie.metrics()["aggregate_acceleration"]["scans"], 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reconcile_failed_refresh_debt_after_restart() {
    // Arrange
    let fixture = amount_fixture(
        "column_batch_failed_refresh",
        "column_batch_failed_refresh",
        &[Value::Int64(2)],
    );
    set_column_batch_maintenance_failure_point(true);
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "INSERT INTO column_batch_failed_refresh (amount) VALUES (8)",
            Vec::new(),
        )
        .expect("row write remains durable");
    let source_rows = fixture
        .cassie
        .midge
        .scan_documents(&fixture.collection)
        .expect("scan source rows");
    assert_eq!(source_rows.len(), 2, "source rows: {source_rows:?}");
    assert!(fixture
        .cassie
        .midge
        .has_column_batch_maintenance_debt(&fixture.collection)
        .expect("read maintenance debt"));

    // Act
    let fallback =
        sum_amount(&fixture, "column_batch_failed_refresh").expect("fall back to exact aggregate");
    let fallback_metrics = fixture.cassie.metrics();
    let restarted = restart_fixture(fixture);
    let recovered =
        sum_amount(&restarted, "column_batch_failed_refresh").expect("aggregate rebuilt summaries");
    let recovered_metrics = restarted.cassie.metrics();

    // Assert
    assert_eq!(fallback, vec![vec![Value::Int64(10)]]);
    assert_eq!(fallback_metrics["aggregate_acceleration"]["scans"], 0);
    assert_eq!(
        fallback_metrics["aggregate_acceleration"]["row_blob_fallbacks"],
        1
    );
    assert_eq!(
        fallback_metrics["column_batches"]["last_fallback_reason"],
        "maintenance_pending"
    );
    assert_eq!(recovered, fallback);
    assert_eq!(recovered_metrics["aggregate_acceleration"]["scans"], 1);
    assert!(!restarted
        .cassie
        .midge
        .has_column_batch_maintenance_debt(&restarted.collection)
        .expect("maintenance debt cleared"));

    let _ = std::fs::remove_dir_all(&restarted.path);
}

#[test]
fn should_rebuild_generation_mismatch_after_restart() {
    // Arrange
    let fixture = amount_fixture(
        "column_batch_generation_mismatch",
        "column_batch_generation_mismatch",
        &[Value::Int64(4), Value::Int64(6)],
    );
    let (key, mut metadata) = metadata_entry(&fixture);
    let stale_generation = metadata["built_generation"]
        .as_u64()
        .expect("built generation")
        .saturating_add(1);
    metadata["built_generation"] = serde_json::json!(stale_generation);
    write_metadata(&fixture, key, &metadata);

    // Act
    let fallback = sum_amount(&fixture, "column_batch_generation_mismatch")
        .expect("fall back to exact aggregate");
    let fallback_metrics = fixture.cassie.metrics();
    let restarted = restart_fixture(fixture);
    let recovered = sum_amount(&restarted, "column_batch_generation_mismatch")
        .expect("aggregate rebuilt summaries");
    let recovered_metrics = restarted.cassie.metrics();

    // Assert
    assert_eq!(fallback, vec![vec![Value::Int64(10)]]);
    assert_eq!(fallback_metrics["aggregate_acceleration"]["scans"], 0);
    assert_eq!(
        fallback_metrics["column_batches"]["last_fallback_reason"],
        "generation_mismatch"
    );
    assert_eq!(recovered, fallback);
    assert_eq!(recovered_metrics["aggregate_acceleration"]["scans"], 1);
    let repaired = restarted
        .cassie
        .midge
        .get_column_batch_metadata(&restarted.collection, &restarted.index)
        .expect("read repaired metadata")
        .expect("repaired metadata");
    assert_eq!(
        repaired.built_generation,
        restarted
            .cassie
            .midge
            .collection_generation(&restarted.collection)
            .expect("current generation")
    );

    let _ = std::fs::remove_dir_all(&restarted.path);
}

#[test]
fn should_report_checked_integer_overflow_without_publishing_acceleration() {
    // Arrange
    let fixture = amount_fixture(
        "column_batch_integer_overflow",
        "column_batch_integer_overflow",
        &[Value::Int64(i64::MAX), Value::Int64(1)],
    );

    // Act
    let accelerated_error = sum_amount(&fixture, "column_batch_integer_overflow")
        .expect_err("accelerated SUM must report overflow");
    let (key, mut metadata) = metadata_entry(&fixture);
    metadata["built_generation"] = serde_json::json!(metadata["built_generation"]
        .as_u64()
        .expect("built generation")
        .saturating_add(1));
    write_metadata(&fixture, key, &metadata);
    let exact_error = sum_amount(&fixture, "column_batch_integer_overflow")
        .expect_err("exact SUM must report overflow");
    let metrics = fixture.cassie.metrics();

    // Assert
    assert!(accelerated_error.contains("aggregate integer overflow"));
    assert_eq!(exact_error, accelerated_error);
    assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
    assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);

    let _ = std::fs::remove_dir_all(&fixture.path);
}

#[test]
fn should_detect_checked_integer_overflow_across_segment_boundaries() {
    // Arrange
    let fixture = ordered_numeric_fixture(
        "column_batch_cross_segment_overflow",
        "column_batch_cross_segment_overflow",
        "BIGINT",
        &[
            serde_json::json!(i64::MAX),
            serde_json::json!(0),
            serde_json::json!(1),
            serde_json::json!(-1),
        ],
        2,
    );

    // Act
    let accelerated = sum_amount(&fixture, "column_batch_cross_segment_overflow")
        .expect_err("summary fold must detect row-order overflow");
    let metrics = fixture.cassie.metrics();
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP INDEX column_batch_cross_segment_overflow_column_idx ON column_batch_cross_segment_overflow",
            vec![],
        )
        .expect("drop column index");
    let exact = sum_amount_as(
        &fixture,
        "column_batch_cross_segment_overflow",
        "exact_total",
    )
    .expect_err("row fold must detect overflow");

    // Assert
    assert_eq!(accelerated, exact);
    assert!(accelerated.contains("aggregate integer overflow"));
    assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);

    let _ = std::fs::remove_dir_all(&fixture.path);
}

#[test]
fn should_not_invent_checked_integer_overflow_inside_a_segment() {
    // Arrange
    let fixture = ordered_numeric_fixture(
        "column_batch_cross_segment_safe_sum",
        "column_batch_cross_segment_safe_sum",
        "BIGINT",
        &[
            serde_json::json!(-1),
            serde_json::json!(0),
            serde_json::json!(i64::MAX),
            serde_json::json!(1),
        ],
        2,
    );

    // Act
    let accelerated = sum_amount(&fixture, "column_batch_cross_segment_safe_sum")
        .expect("incoming sum keeps the segment fold in range");
    let metrics = fixture.cassie.metrics();
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP INDEX column_batch_cross_segment_safe_sum_column_idx ON column_batch_cross_segment_safe_sum",
            vec![],
        )
        .expect("drop column index");
    let exact = sum_amount_as(
        &fixture,
        "column_batch_cross_segment_safe_sum",
        "exact_total",
    )
    .expect("exact row fold");

    // Assert
    assert_eq!(accelerated, vec![vec![Value::Int64(i64::MAX)]]);
    assert_eq!(exact, accelerated);
    assert_eq!(metrics["aggregate_acceleration"]["scans"], 1);

    let _ = std::fs::remove_dir_all(&fixture.path);
}

#[test]
fn should_fallback_when_floating_summaries_cannot_preserve_row_order() {
    // Arrange
    let fixture = ordered_numeric_fixture(
        "column_batch_float_row_order",
        "column_batch_float_row_order",
        "FLOAT",
        &[
            serde_json::json!(10_000_000_000_000_000.0),
            serde_json::json!(0.0),
            serde_json::json!(-10_000_000_000_000_000.0),
            serde_json::json!(1.0),
        ],
        2,
    );

    // Act
    let fallback = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT SUM(amount) AS fallback_sum, AVG(amount) AS fallback_avg FROM column_batch_float_row_order",
            vec![],
        )
        .expect("fall back to row-order floating aggregates");
    let metrics = fixture.cassie.metrics();
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP INDEX column_batch_float_row_order_column_idx ON column_batch_float_row_order",
            vec![],
        )
        .expect("drop column index");
    let exact = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT SUM(amount) AS exact_sum, AVG(amount) AS exact_avg FROM column_batch_float_row_order",
            vec![],
        )
        .expect("run exact floating aggregates");

    // Assert
    assert_eq!(
        fallback.rows,
        vec![vec![Value::Float64(1.0), Value::Float64(0.25)]]
    );
    assert_eq!(exact.rows, fallback.rows);
    assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
    assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);
    assert_eq!(
        metrics["column_batches"]["last_fallback_reason"],
        "numeric_summary_requires_rows"
    );

    let _ = std::fs::remove_dir_all(&fixture.path);
}

#[test]
fn should_rebuild_old_metadata_format_after_restart() {
    // Arrange
    let fixture = amount_fixture(
        "column_batch_metadata_format",
        "column_batch_metadata_format",
        &[Value::Int64(3), Value::Int64(7)],
    );
    let (key, mut metadata) = metadata_entry(&fixture);
    metadata["metadata_format_version"] = serde_json::json!(0);
    write_metadata(&fixture, key, &metadata);

    // Act
    let fallback = sum_amount(&fixture, "column_batch_metadata_format")
        .expect("old metadata format falls back");
    let metrics = fixture.cassie.metrics();
    let repaired = restart_fixture(fixture);
    let recovered =
        sum_amount(&repaired, "column_batch_metadata_format").expect("metadata format rebuilt");

    // Assert
    assert_eq!(fallback, vec![vec![Value::Int64(10)]]);
    assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
    assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);
    assert_eq!(
        metrics["column_batches"]["last_fallback_reason"],
        "metadata_format_mismatch"
    );
    assert_eq!(recovered, fallback);
    assert_eq!(
        repaired.cassie.metrics()["aggregate_acceleration"]["scans"],
        1
    );

    let _ = std::fs::remove_dir_all(&repaired.path);
}

#[test]
fn should_reconcile_invalid_summary_formats_after_restart() {
    // Arrange
    let fixture = amount_fixture(
        "column_batch_summary_recovery",
        "column_batch_summary_recovery",
        &[Value::Int64(3), Value::Int64(7)],
    );
    let (key, mut metadata) = metadata_entry(&fixture);
    metadata["summary_format_version"] = serde_json::json!(0);
    write_metadata(&fixture, key, &metadata);

    // Act
    let old_format = sum_amount_as(&fixture, "column_batch_summary_recovery", "old_total")
        .expect("old format falls back");
    let old_metrics = fixture.cassie.metrics();
    let repaired = restart_fixture(fixture);
    let repaired_rows = sum_amount_as(&repaired, "column_batch_summary_recovery", "repaired_total")
        .expect("old format rebuilt");
    let (key, mut metadata) = metadata_entry(&repaired);
    metadata["segments"][0]["summaries"]["amount"]
        .as_object_mut()
        .expect("amount summary")
        .remove("sum");
    write_metadata(&repaired, key, &metadata);
    let malformed = sum_amount_as(
        &repaired,
        "column_batch_summary_recovery",
        "malformed_total",
    )
    .expect("malformed summary falls back");
    let malformed_metrics = repaired.cassie.metrics();
    let repaired = restart_fixture(repaired);
    let (key, mut metadata) = metadata_entry(&repaired);
    metadata["segments"][0]["summaries"]["amount"]["non_null_count"] = serde_json::json!(99);
    write_metadata(&repaired, key, &metadata);
    let bad_checksum = sum_amount_as(&repaired, "column_batch_summary_recovery", "checksum_total")
        .expect("inconsistent summary falls back");
    let checksum_metrics = repaired.cassie.metrics();
    let repaired = restart_fixture(repaired);
    let final_rows = sum_amount_as(&repaired, "column_batch_summary_recovery", "final_total")
        .expect("inconsistent summary rebuilt");

    // Assert
    let expected = vec![vec![Value::Int64(10)]];
    assert_eq!(old_format, expected);
    assert_eq!(
        old_metrics["column_batches"]["last_fallback_reason"],
        "summary_format_mismatch"
    );
    assert_eq!(repaired_rows, expected);
    assert_eq!(malformed, expected);
    assert_eq!(
        malformed_metrics["column_batches"]["last_fallback_reason"],
        "invalid_metadata"
    );
    assert_eq!(bad_checksum, expected);
    assert_eq!(
        checksum_metrics["column_batches"]["last_fallback_reason"],
        "summary_checksum_mismatch"
    );
    assert_eq!(final_rows, expected);
    assert_eq!(
        repaired.cassie.metrics()["aggregate_acceleration"]["scans"],
        1
    );

    let _ = std::fs::remove_dir_all(&repaired.path);
}

#[test]
fn should_rebuild_source_count_mismatch_after_restart() {
    // Arrange
    let fixture = amount_fixture(
        "column_batch_source_count",
        "column_batch_source_count",
        &[Value::Int64(2), Value::Int64(5)],
    );
    let (key, mut metadata) = metadata_entry(&fixture);
    metadata["source_row_count"] = serde_json::json!(3);
    write_metadata(&fixture, key, &metadata);

    // Act
    let fallback = sum_amount(&fixture, "column_batch_source_count")
        .expect("source count mismatch falls back");
    let metrics = fixture.cassie.metrics();
    let repaired = restart_fixture(fixture);
    let recovered =
        sum_amount(&repaired, "column_batch_source_count").expect("source count rebuilt");

    // Assert
    assert_eq!(fallback, vec![vec![Value::Int64(7)]]);
    assert_eq!(
        metrics["column_batches"]["last_fallback_reason"],
        "source_row_count_mismatch"
    );
    assert_eq!(recovered, fallback);
    assert_eq!(
        repaired.cassie.metrics()["aggregate_acceleration"]["scans"],
        1
    );

    let _ = std::fs::remove_dir_all(&repaired.path);
}

#[test]
fn should_fallback_when_any_segment_is_missing_and_rebuild_after_restart() {
    // Arrange
    let fixture = amount_fixture(
        "column_batch_missing_segment",
        "column_batch_missing_segment",
        &[Value::Int64(1), Value::Int64(2), Value::Int64(3)],
    );
    let segment_key = fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan column segments")
        .into_iter()
        .find_map(|(key, value)| value.starts_with(b"CCB1").then_some(key))
        .expect("persisted segment");
    fixture
        .cassie
        .midge
        .raw_delete(StorageFamily::Data, &segment_key)
        .expect("remove segment");

    // Act
    let fallback =
        sum_amount(&fixture, "column_batch_missing_segment").expect("missing segment falls back");
    let metrics = fixture.cassie.metrics();
    let repaired = restart_fixture(fixture);
    let recovered =
        sum_amount(&repaired, "column_batch_missing_segment").expect("missing segment rebuilt");

    // Assert
    assert_eq!(fallback, vec![vec![Value::Int64(6)]]);
    assert_eq!(
        metrics["column_batches"]["last_fallback_reason"],
        "segment_missing"
    );
    assert_eq!(recovered, fallback);
    assert_eq!(
        repaired.cassie.metrics()["aggregate_acceleration"]["scans"],
        1
    );

    let _ = std::fs::remove_dir_all(&repaired.path);
}

#[test]
fn should_preserve_all_null_aggregate_semantics() {
    // Arrange
    let fixture = amount_fixture(
        "column_batch_all_null",
        "column_batch_all_null",
        &[Value::Null, Value::Null],
    );

    // Act
    let result = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT COUNT(*) AS rows, COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM column_batch_all_null",
            vec![],
        )
        .expect("aggregate null rows");

    // Assert
    assert_eq!(
        result.rows,
        vec![vec![
            Value::Int64(2),
            Value::Int64(0),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ]]
    );
    assert_eq!(
        fixture.cassie.metrics()["aggregate_acceleration"]["scans"],
        1
    );

    let _ = std::fs::remove_dir_all(&fixture.path);
}

#[test]
fn should_preserve_zero_row_aggregate_semantics() {
    // Arrange
    let fixture = amount_fixture("column_batch_zero_rows", "column_batch_zero_rows", &[]);

    // Act
    let result = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT COUNT(*) AS rows, COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM column_batch_zero_rows",
            vec![],
        )
        .expect("aggregate empty table");
    let metrics = fixture.cassie.metrics();

    // Assert
    assert_eq!(
        result.rows,
        vec![vec![
            Value::Int64(0),
            Value::Int64(0),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ]]
    );
    assert_eq!(metrics["aggregate_acceleration"]["scans"], 1);
    assert_eq!(metrics["aggregate_acceleration"]["accelerated_segments"], 0);

    let _ = std::fs::remove_dir_all(&fixture.path);
}

#[test]
fn should_preserve_mixed_numeric_summary_semantics() {
    // Arrange
    let fixture = ordered_numeric_fixture(
        "column_batch_mixed_numerics",
        "column_batch_mixed_numerics",
        "JSON",
        &[
            serde_json::json!(2),
            serde_json::json!(1.5),
            serde_json::json!(-1),
            serde_json::Value::Null,
        ],
        8,
    );

    // Act
    let accelerated = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT SUM(amount) AS accelerated_sum, AVG(amount) AS accelerated_avg, MIN(amount) AS accelerated_min, MAX(amount) AS accelerated_max FROM column_batch_mixed_numerics",
            vec![],
        )
        .expect("aggregate mixed numeric summary");
    let accelerated_metrics = fixture.cassie.metrics();
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP INDEX column_batch_mixed_numerics_column_idx ON column_batch_mixed_numerics",
            vec![],
        )
        .expect("drop column index");
    let exact = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT SUM(amount) AS exact_sum, AVG(amount) AS exact_avg, MIN(amount) AS exact_min, MAX(amount) AS exact_max FROM column_batch_mixed_numerics",
            vec![],
        )
        .expect("aggregate mixed numerics from rows");

    // Assert
    assert_eq!(
        accelerated.rows,
        vec![vec![
            Value::Float64(2.5),
            Value::Float64(2.5 / 3.0),
            Value::Int64(-1),
            Value::Int64(2),
        ]]
    );
    assert_eq!(exact.rows, accelerated.rows);
    assert_eq!(accelerated_metrics["aggregate_acceleration"]["scans"], 1);

    let _ = std::fs::remove_dir_all(&fixture.path);
}

#[test]
fn should_fallback_when_typed_minmax_cannot_match_row_comparison() {
    // Arrange
    let fixture = ordered_numeric_fixture(
        "column_batch_typed_vector_minmax",
        "column_batch_typed_vector_minmax",
        "VECTOR(2)",
        &[
            serde_json::json!([1.0, 2.0]),
            serde_json::json!([0.0, 10.0]),
        ],
        2,
    );

    // Act
    let accelerated = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT MIN(amount) AS accelerated_min, MAX(amount) AS accelerated_max FROM column_batch_typed_vector_minmax",
            vec![],
        )
        .expect("aggregate typed vector summaries");
    let metrics = fixture.cassie.metrics();
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP INDEX column_batch_typed_vector_minmax_column_idx ON column_batch_typed_vector_minmax",
            vec![],
        )
        .expect("drop column index");
    let exact = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT MIN(amount) AS exact_min, MAX(amount) AS exact_max FROM column_batch_typed_vector_minmax",
            vec![],
        )
        .expect("aggregate vectors from rows");

    // Assert
    assert_eq!(
        accelerated.rows,
        vec![vec![
            Value::String("[0,10]".to_string()),
            Value::String("[1,2]".to_string()),
        ]]
    );
    assert_eq!(exact.rows, accelerated.rows);
    assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
    assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);
    assert_eq!(
        metrics["column_batches"]["last_fallback_reason"],
        "typed_summary_requires_rows"
    );

    let _ = std::fs::remove_dir_all(&fixture.path);
}
