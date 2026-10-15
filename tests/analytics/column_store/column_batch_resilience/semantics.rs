#[test]
fn should_preserve_integer_sum_above_two_to_the_fifty_third() {
    // Arrange
    let _test_guard = COLUMN_BATCH_FAILPOINT_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    use_local_storage();
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
    assert_eq!(
        result.rows(),
        vec![vec![Value::Int64(9_007_199_254_740_995)]]
    );
    assert_eq!(cassie.metrics()["aggregate_acceleration"]["scans"], 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reconcile_existing_maintenance_debt_when_publishing_column_index() {
    // Arrange
    let _test_guard = COLUMN_BATCH_FAILPOINT_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    use_local_storage();
    let path = data_dir("column_batch_publication_debt");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE column_batch_publication_debt (amount BIGINT)",
            vec![],
        )
        .expect("create table");
    let collection = canonical_test_collection(&cassie, "column_batch_publication_debt");
    set_column_batch_maintenance_failure_point(true);
    cassie
        .execute_sql(
            &session,
            "INSERT INTO column_batch_publication_debt (amount) VALUES (8)",
            vec![],
        )
        .expect("row write remains durable");
    assert!(cassie
        .midge
        .has_column_batch_maintenance_debt(&collection)
        .expect("read maintenance debt"));

    // Act
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX column_batch_publication_debt_idx ON column_batch_publication_debt USING column (amount) WITH (segment_size = 1)",
            vec![],
        )
        .expect("publish column index");
    let result = cassie
        .execute_sql(
            &session,
            "SELECT SUM(amount) AS total FROM column_batch_publication_debt",
            vec![],
        )
        .expect("read published column index");

    // Assert
    assert!(!cassie
        .midge
        .has_column_batch_maintenance_debt(&collection)
        .expect("maintenance debt cleared"));
    assert_eq!(result.rows(), vec![vec![Value::Int64(8)]]);
    assert_eq!(cassie.metrics()["aggregate_acceleration"]["scans"], 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_alp_chunk_lifecycle_through_rename_drop() {
    // Arrange
    let values = (0..128)
        .map(|position| serde_json::json!((f64::from(position) - 64.0) / 100.0))
        .collect::<Vec<_>>();
    let fixture = ordered_numeric_fixture(
        "column_batch_alp_lifecycle",
        "column_batch_alp_lifecycle_before",
        "FLOAT",
        &values,
        128,
    );
    let artifact_count = |cassie: &Cassie| {
        cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("scan column-batch artifacts")
            .into_iter()
            .filter(|(_, value)| {
                value.starts_with(b"CBM2")
                    || value.starts_with(b"CBR2")
                    || value.starts_with(b"CBC2")
            })
            .count()
    };
    let original = fixture
        .cassie
        .midge
        .get_column_batch_metadata(
            "column_batch_alp_lifecycle_before",
            "column_batch_alp_lifecycle_before_column_idx",
        )
        .expect("read original ALP metadata")
        .expect("original ALP metadata");
    assert_eq!(
        original.segments[0].field_chunks["amount"].codec_name,
        "alp"
    );
    let original_artifact_count = artifact_count(&fixture.cassie);

    // Act
    fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "ALTER TABLE column_batch_alp_lifecycle_before RENAME TO column_batch_alp_lifecycle_after",
                vec![],
            )
            .expect("rename table with ALP chunks");
    let renamed = fixture
        .cassie
        .midge
        .get_column_batch_metadata(
            "column_batch_alp_lifecycle_after",
            "column_batch_alp_lifecycle_before_column_idx",
        )
        .expect("read renamed ALP metadata")
        .expect("renamed ALP metadata");
    let selected = fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "SELECT amount FROM column_batch_alp_lifecycle_after WHERE amount >= 0.60 ORDER BY amount",
                vec![],
            )
            .expect("query renamed ALP chunks");
    let renamed_artifact_count = artifact_count(&fixture.cassie);
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP TABLE column_batch_alp_lifecycle_after",
            vec![],
        )
        .expect("drop table with ALP chunks");
    let dropped_artifact_count = artifact_count(&fixture.cassie);

    // Assert
    assert_eq!(
        renamed.collection,
        "postgres.public.column_batch_alp_lifecycle_after"
    );
    assert_eq!(renamed.segments[0].field_chunks["amount"].codec_name, "alp");
    assert_eq!(selected.rows().len(), 4);
    assert_eq!(renamed_artifact_count, original_artifact_count);
    assert!(original_artifact_count >= 3);
    assert_eq!(dropped_artifact_count, 0);

    let _ = std::fs::remove_dir_all(&fixture.path);
}

#[test]
fn should_preserve_fsst_chunk_lifecycle_through_rename_drop() {
    // Arrange
    let common = "account-lifecycle-event-payload/".repeat(6);
    let values = (0..128)
        .map(|position| serde_json::json!(format!("prefix-{position:04}-{common}")))
        .collect::<Vec<_>>();
    let fixture = ordered_numeric_fixture(
        "column_batch_fsst_lifecycle",
        "column_batch_fsst_lifecycle_before",
        "TEXT",
        &values,
        128,
    );
    let artifact_count = |cassie: &Cassie| {
        cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("scan column-batch artifacts")
            .into_iter()
            .filter(|(_, value)| {
                value.starts_with(b"CBM2")
                    || value.starts_with(b"CBR2")
                    || value.starts_with(b"CBC2")
            })
            .count()
    };
    let original = fixture
        .cassie
        .midge
        .get_column_batch_metadata(
            "column_batch_fsst_lifecycle_before",
            "column_batch_fsst_lifecycle_before_column_idx",
        )
        .expect("read original FSST metadata")
        .expect("original FSST metadata");
    assert_eq!(
        original.segments[0].field_chunks["amount"].codec_name,
        "fsst"
    );
    let original_artifact_count = artifact_count(&fixture.cassie);

    // Act
    fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "ALTER TABLE column_batch_fsst_lifecycle_before RENAME TO column_batch_fsst_lifecycle_after",
                vec![],
            )
            .expect("rename table with FSST chunks");
    let renamed = fixture
        .cassie
        .midge
        .get_column_batch_metadata(
            "column_batch_fsst_lifecycle_after",
            "column_batch_fsst_lifecycle_before_column_idx",
        )
        .expect("read renamed FSST metadata")
        .expect("renamed FSST metadata");
    let selected = fsst_codec_rows(
        &fixture,
        "column_batch_fsst_lifecycle_after",
        "amount >= 'prefix-0120-'",
    );
    let renamed_artifact_count = artifact_count(&fixture.cassie);
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP TABLE column_batch_fsst_lifecycle_after",
            vec![],
        )
        .expect("drop table with FSST chunks");
    let dropped_artifact_count = artifact_count(&fixture.cassie);

    // Assert
    assert_eq!(
        renamed.collection,
        "postgres.public.column_batch_fsst_lifecycle_after"
    );
    assert_eq!(
        renamed.segments[0].field_chunks["amount"].codec_name,
        "fsst"
    );
    assert_eq!(selected.len(), 8);
    assert_eq!(renamed_artifact_count, original_artifact_count);
    assert!(original_artifact_count >= 3);
    assert_eq!(dropped_artifact_count, 0);

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
        fallback.rows(),
        vec![vec![Value::Float64(1.0), Value::Float64(0.25)]]
    );
    assert_eq!(exact.rows(), fallback.rows());
    assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
    assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);
    assert_eq!(
        metrics["column_batches"]["last_fallback_reason"],
        "numeric_summary_requires_rows"
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
        result.rows(),
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
fn should_preserve_mixed_numeric_row_fallback_semantics() {
    // Arrange
    let fixture = ordered_numeric_fixture(
        "column_batch_mixed_numerics",
        "column_batch_mixed_numerics",
        "JSON",
        &[
            serde_json::json!(2),
            serde_json::json!(1.5),
            serde_json::json!(-1),
        ],
        8,
    );
    // An absent JSON field represents SQL NULL. An explicit JSON null is
    // now a present value and is not silently ignored by numeric aggregates.
    fixture
        .cassie
        .midge
        .put_document(
            &fixture.collection,
            Some("row-0003".to_string()),
            serde_json::json!({}),
        )
        .expect("insert SQL NULL as an absent JSON field");

    // Act
    let fallback = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT SUM(amount) AS fallback_sum, AVG(amount) AS fallback_avg, MIN(amount) AS fallback_min, MAX(amount) AS fallback_max FROM column_batch_mixed_numerics",
            vec![],
        )
        .expect("aggregate mixed numerics from rows");
    let fallback_metrics = fixture.cassie.metrics();
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
        fallback.rows(),
        vec![vec![
            Value::Float64(2.5),
            Value::Float64(2.5 / 3.0),
            Value::Int64(-1),
            Value::Int64(2),
        ]]
    );
    assert_eq!(exact.rows(), fallback.rows());
    assert_eq!(fallback_metrics["aggregate_acceleration"]["scans"], 0);
    assert_eq!(
        fallback_metrics["aggregate_acceleration"]["row_blob_fallbacks"],
        1
    );

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
        accelerated.rows(),
        vec![vec![
            Value::Vector(cassie::types::Vector::new(vec![0.0, 10.0])),
            Value::Vector(cassie::types::Vector::new(vec![1.0, 2.0])),
        ]]
    );
    assert_eq!(exact.rows(), accelerated.rows());
    assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
    assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);
    assert_eq!(
        metrics["column_batches"]["last_fallback_reason"],
        "typed_summary_requires_rows"
    );

    let _ = std::fs::remove_dir_all(&fixture.path);
}
