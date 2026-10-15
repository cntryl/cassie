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
    metadata.built_generation = metadata.built_generation.saturating_add(1);
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
