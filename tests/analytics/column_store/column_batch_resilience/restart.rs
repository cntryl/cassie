#[test]
fn should_reconcile_failed_refresh_debt_after_restart() {
    // Arrange
    let fixture = persistent_amount_fixture(
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
    let fixture = persistent_amount_fixture(
        "column_batch_generation_mismatch",
        "column_batch_generation_mismatch",
        &[Value::Int64(4), Value::Int64(6)],
    );
    let (key, mut metadata) = metadata_entry(&fixture);
    metadata.built_generation = metadata.built_generation.saturating_add(1);
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
fn should_rebuild_truncated_fsst_chunk_after_restart() {
    // Arrange
    let common = "account-lifecycle-event-payload/".repeat(6);
    let values = (0..128)
        .map(|position| {
            if position % 73 == 0 {
                serde_json::Value::Null
            } else {
                serde_json::json!(format!("prefix-{position:04}-{common}"))
            }
        })
        .collect::<Vec<_>>();
    let fixture = persistent_ordered_numeric_fixture(
        "column_batch_fsst_restart",
        "column_batch_fsst_restart",
        "TEXT",
        &values,
        128,
    );
    let metadata = fixture
        .cassie
        .midge
        .get_column_batch_metadata(&fixture.collection, &fixture.index)
        .expect("read FSST metadata")
        .expect("FSST metadata");
    assert_eq!(
        metadata.segments[0].field_chunks["amount"].codec_name,
        "fsst"
    );
    let (chunk_key, mut chunk_bytes) = fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan FSST chunks")
        .into_iter()
        .find(|(_, value)| value.starts_with(b"CBC1"))
        .expect("persisted FSST chunk");
    chunk_bytes.pop().expect("non-empty FSST chunk");

    // Act
    write_raw_value(&fixture, chunk_key, chunk_bytes);
    let fallback = fsst_codec_rows(
        &fixture,
        "column_batch_fsst_restart",
        "amount >= 'prefix-0116-'",
    );
    let fallback_nulls = fsst_codec_rows(&fixture, "column_batch_fsst_restart", "amount IS NULL");
    let fallback_metrics = fixture.cassie.metrics();
    let restarted = restart_fixture(fixture);
    let recovered = fsst_codec_rows(
        &restarted,
        "column_batch_fsst_restart",
        "amount >= 'prefix-0116-'",
    );
    let recovered_nulls =
        fsst_codec_rows(&restarted, "column_batch_fsst_restart", "amount IS NULL");
    let repaired = restarted
        .cassie
        .midge
        .get_column_batch_metadata(&restarted.collection, &restarted.index)
        .expect("read repaired FSST metadata")
        .expect("repaired FSST metadata");

    // Assert
    assert_eq!(fallback, recovered);
    assert_eq!(fallback.len(), 12);
    assert_eq!(fallback_nulls, recovered_nulls);
    assert_eq!(fallback_nulls, vec![vec![Value::Null]; 2]);
    assert_eq!(fallback_metrics["column_batches"]["scans"], 0);
    assert_eq!(fallback_metrics["column_batches"]["fallback_scans"], 2);
    assert_eq!(
        fallback_metrics["column_batches"]["last_fallback_reason"],
        "segment_checksum_mismatch"
    );
    assert_eq!(
        repaired.segments[0].field_chunks["amount"].codec_name,
        "fsst"
    );

    let _ = std::fs::remove_dir_all(&restarted.path);
}

#[test]
fn should_rebuild_old_metadata_format_after_restart() {
    // Arrange
    let fixture = persistent_amount_fixture(
        "column_batch_metadata_format",
        "column_batch_metadata_format",
        &[Value::Int64(3), Value::Int64(7)],
    );
    let (key, _) = metadata_entry(&fixture);
    write_raw_value(&fixture, key, b"CCB1-derived-v1".to_vec());

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
    let fixture = persistent_amount_fixture(
        "column_batch_summary_recovery",
        "column_batch_summary_recovery",
        &[Value::Int64(3), Value::Int64(7)],
    );
    let (key, metadata) = metadata_entry(&fixture);
    let mut raw =
        encode_column_batch_manifest_for_test(&metadata).expect("encode current manifest");
    raw[6..8].copy_from_slice(&0_u16.to_le_bytes());
    write_raw_value(&fixture, key, resign_manifest(raw));

    // Act
    let old_format = sum_amount_as(&fixture, "column_batch_summary_recovery", "old_total")
        .expect("old format falls back");
    let old_metrics = fixture.cassie.metrics();
    let repaired = restart_fixture(fixture);
    let repaired_rows = sum_amount_as(&repaired, "column_batch_summary_recovery", "repaired_total")
        .expect("old format rebuilt");
    let (key, _) = metadata_entry(&repaired);
    let mut malformed = b"CBM1".to_vec();
    malformed.extend_from_slice(&1_u16.to_le_bytes());
    malformed.extend_from_slice(&1_u16.to_le_bytes());
    malformed.extend_from_slice(&[0_u8; 8]);
    malformed.extend_from_slice(Sha256::digest(malformed.as_slice()).as_slice());
    write_raw_value(&repaired, key, malformed);
    let malformed = sum_amount_as(
        &repaired,
        "column_batch_summary_recovery",
        "malformed_total",
    )
    .expect("malformed summary falls back");
    let malformed_metrics = repaired.cassie.metrics();
    let repaired = restart_fixture(repaired);
    let (key, mut metadata) = metadata_entry(&repaired);
    metadata.segments[0]
        .summaries
        .get_mut("amount")
        .expect("amount summary")
        .non_null_count = 99;
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
    let fixture = persistent_amount_fixture(
        "column_batch_source_count",
        "column_batch_source_count",
        &[Value::Int64(2), Value::Int64(5)],
    );
    let (key, mut metadata) = metadata_entry(&fixture);
    metadata.source_row_count = 3;
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
    let fixture = persistent_amount_fixture(
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
        .find_map(|(key, value)| value.starts_with(b"CBR1").then_some(key))
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
