#[test]
fn should_recover_from_truncated_or_unknown_version_alp_chunks() {
    // Arrange
    let values = (0..128)
        .map(|position| {
            if position % 73 == 0 {
                serde_json::Value::Null
            } else {
                serde_json::json!((f64::from(position) - 256.0) / 100.0)
            }
        })
        .collect::<Vec<_>>();
    let fixture = persistent_ordered_numeric_fixture(
        "column_batch_alp_codec_version",
        "column_batch_alp_codec_version",
        "FLOAT",
        &values,
        128,
    );
    let (key, mut metadata) = metadata_entry(&fixture);
    let chunk = metadata.segments[0]
        .field_chunks
        .get_mut("amount")
        .expect("ALP amount chunk");
    assert_eq!(chunk.codec_name, "alp");
    assert_eq!(chunk.codec_version, 1);
    chunk.codec_version = 2;
    let (chunk_key, chunk_bytes) = fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan ALP chunks")
        .into_iter()
        .find(|(_, value)| value.starts_with(b"CBC2"))
        .expect("persisted ALP chunk");
    let mut truncated = chunk_bytes.clone();
    truncated.pop().expect("non-empty ALP chunk");

    // Act
    write_raw_value(&fixture, chunk_key.clone(), truncated);
    let corrupt_fallback = alp_codec_rows(&fixture, "amount >= -2.00");
    let corrupt_metrics = fixture.cassie.metrics();
    write_raw_value(&fixture, chunk_key, chunk_bytes);
    write_metadata(&fixture, key, &metadata);
    let version_fallback = alp_codec_rows(&fixture, "amount >= -2.00");
    let fallback_nulls = alp_codec_rows(&fixture, "amount IS NULL");
    let fallback_metrics = fixture.cassie.metrics();
    let restarted = restart_fixture(fixture);
    let recovered = alp_codec_rows(&restarted, "amount >= -2.00");
    let recovered_nulls = alp_codec_rows(&restarted, "amount IS NULL");
    let repaired = restarted
        .cassie
        .midge
        .get_column_batch_metadata(&restarted.collection, &restarted.index)
        .expect("read repaired metadata")
        .expect("repaired metadata");
    let accelerated_non_exact = alp_codec_rows(&restarted, "amount > -2.00");
    restarted
            .cassie
            .execute_sql(
                &restarted.session,
                "DROP INDEX column_batch_alp_codec_version_column_idx ON column_batch_alp_codec_version",
                vec![],
            )
            .expect("drop rebuilt ALP index");
    let exact_non_exact = alp_codec_rows(&restarted, "amount > -2.00");

    // Assert
    assert_eq!(corrupt_fallback, recovered);
    assert_eq!(version_fallback, recovered);
    assert_eq!(version_fallback.len(), 71);
    assert_eq!(fallback_nulls, recovered_nulls);
    assert_eq!(fallback_nulls, vec![vec![Value::Null]; 2]);
    assert_eq!(accelerated_non_exact, exact_non_exact);
    assert_eq!(corrupt_metrics["column_batches"]["scans"], 0);
    assert_eq!(corrupt_metrics["column_batches"]["fallback_scans"], 1);
    assert_eq!(
        corrupt_metrics["column_batches"]["last_fallback_reason"],
        "segment_checksum_mismatch"
    );
    assert_eq!(fallback_metrics["column_batches"]["scans"], 0);
    assert_eq!(fallback_metrics["column_batches"]["fallback_scans"], 2);
    assert_eq!(
        fallback_metrics["column_batches"]["last_fallback_reason"],
        "segment_codec_mismatch"
    );
    let repaired_chunk = repaired.segments[0]
        .field_chunks
        .get("amount")
        .expect("repaired ALP amount chunk");
    assert_eq!(repaired_chunk.codec_name, "alp");
    assert_eq!(repaired_chunk.codec_version, 1);

    let _ = std::fs::remove_dir_all(&restarted.path);
}
