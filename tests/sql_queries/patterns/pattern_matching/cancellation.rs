#[test]
fn should_require_finite_regex_execution_before_active_cancellation() {
    // Arrange
    // #799 must implement regex execution before #954 can synchronize active cancellation.
    let fixture = sql_fixture("issue-799-regex-cancellation-prerequisite", &[]);

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 3 AND n::TEXT ~ '^[0-9]+$') SELECT MAX(n) FROM seq",
    );

    // Assert
    assert_eq!(result.expect("regex execution prerequisite").rows(), &[vec![Value::Int64(3)]]);
}

#[test]
fn should_cancel_active_regex_work_before_output_publication() {
    // Arrange
    let sql = "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 3 AND n::TEXT ~ '^[0-9]+$') SELECT MAX(n) FROM seq";

    // Act
    let result = crate::support_regex_cancellation::run(sql);

    // Assert
    let wire = &result.cancelled;
    assert_eq!(crate::support_pgwire::error_code(wire).as_deref(), Some("57014"));
    assert_eq!(
        crate::support_pgwire::data_rows(wire),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(wire.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(wire.last().map(|frame| frame.1.as_slice()), Some(b"I".as_slice()));
    assert_eq!(result.memory_after, result.memory_before);
    assert_eq!(crate::support_pgwire::error_code(&result.recovery), None);
    assert_eq!(
        crate::support_pgwire::data_rows(&result.recovery),
        crate::support_pgwire::data_rows(&result.control)
    );
    assert_eq!(
        crate::support_pgwire::data_rows(&result.recovery),
        vec![vec![Some("3".to_owned())]]
    );
}

#[test]
fn should_cancel_active_are_constraint_work_before_output_publication() {
    // Arrange
    let sql = "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 3 AND n::TEXT ~ '^(?=[0-9])[0-9]+$') SELECT MAX(n) FROM seq";

    // Act
    let result = crate::support_regex_cancellation::run(sql);

    // Assert
    let wire = &result.cancelled;
    assert_eq!(crate::support_pgwire::error_code(wire).as_deref(), Some("57014"));
    assert_eq!(
        crate::support_pgwire::data_rows(wire),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(wire.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(wire.last().map(|frame| frame.1.as_slice()), Some(b"I".as_slice()));
    assert_eq!(result.memory_after, result.memory_before);
    assert_eq!(crate::support_pgwire::error_code(&result.recovery), None);
    assert_eq!(crate::support_pgwire::data_rows(&result.recovery), crate::support_pgwire::data_rows(&result.control));
    assert_eq!(crate::support_pgwire::data_rows(&result.recovery), vec![vec![Some("3".to_owned())]]);
}

#[test]
fn should_apply_pattern_producer_limits_to_rollup_filters_before_publication() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("config");
    config.limits.query_memory_budget_bytes = 64 * 1024;
    let fixture = sql_fixture_with_config("rollup-pattern-producer", &[
        "CREATE TABLE pattern_source(tenant TEXT,event_at TEXT)",
        "INSERT INTO pattern_source VALUES('a','2026-01-01T00:05:00Z')",
    ], config);
    let prefix = "CREATE ROLLUP pattern_rollup ON pattern_source USING time_bucket('1 hour',event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total WHERE ";
    // Act
    let rejected = fixture.execute(&format!("{prefix}regexp_like(repeat(tenant,65537),'a')"));
    let absent = fixture.cassie.catalog.get_rollup("pattern_rollup").is_none();
    let recovery = fixture.rows("SELECT 1");
    let retry = fixture.execute(&format!("{prefix}regexp_like(tenant,'a')"));
    // Assert
    assert!(matches!(rejected, Err(CassieError::ResourceLimit(_))), "{rejected:?}");
    assert!(absent);
    assert_eq!(recovery, vec![vec![Value::Int64(1)]]);
    retry.expect("bounded retry can reuse the rollup name");
    assert_eq!(current_accounted_memory_bytes(&fixture), 0);
}
