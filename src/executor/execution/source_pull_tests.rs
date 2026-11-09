//! Bounded executable witnesses for consumer stop, cancellation and ownership.
use crate::app::{Cassie, CassieError};
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use crate::runtime::{ExecutionMode, QueryCancellationHandle, QueryExecutionControls};
use crate::types::Value;
use std::time::Instant;

fn fixture() -> Cassie {
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_in_memory_with_config(config).expect("in-memory cursor fixture");
    cassie.startup().expect("startup");
    cassie
}

#[test]
fn should_stop_unnest_given_a_filtered_limit_when_two_rows_satisfy_the_consumer() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );

    // Act
    let (result, advances) = super::source_set_function::probe::run(None, || {
        cassie.execute_sql_with_controls(
            &session,
            "SELECT n + 10 FROM unnest(ARRAY[1,2,3,4,5]) s(n) WHERE n > 1 LIMIT 1",
            vec![],
            ExecutionMode::SimpleQuery,
            &controls,
        )
    });

    // Assert
    let result = result.expect("filtered pull result");
    assert_eq!(result.rows(), &[vec![Value::Int64(12)]]);
    assert_eq!(
        advances, 2,
        "the predicate must consume only its rejected first row and accepted second row"
    );
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_unnest_at_advancement_without_retaining_partial_results() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let (result, advances) =
        super::source_set_function::probe::run(Some((2, cancellation)), || {
            cassie.execute_sql_with_controls(
                &session,
                "SELECT n FROM unnest(ARRAY[1,2,3,4,5]) s(n)",
                vec![],
                ExecutionMode::SimpleQuery,
                &controls,
            )
        });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(advances, 2);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(
        cassie
            .execute_sql(&session, "SELECT 1", vec![])
            .expect("connection recovers")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_stop_generated_rows_at_the_requested_consumer_limit() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    let queries = [
        "SELECT n FROM (SELECT n FROM unnest(ARRAY[1,2,3,4,5]) s(n) WHERE n > 1) d WHERE n < 4 LIMIT 1",
        "SELECT s.n FROM (VALUES (1)) v(k) CROSS JOIN LATERAL unnest(ARRAY[1,2,3,4,5]) s(n) WHERE s.n > v.k LIMIT 1",
        "WITH source AS NOT MATERIALIZED (SELECT n FROM unnest(ARRAY[1,2,3,4,5]) s(n)) SELECT n FROM source WHERE n > 1 LIMIT 1",
        "SELECT n FROM generate_series(1,2147483647) s(n) WHERE n > 1 LIMIT 1",
        "SELECT n FROM (SELECT n FROM generate_series(1,2147483647) s(n) WHERE n > 1) d LIMIT 1",
    ];

    for sql in queries {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        // Act
        let (result, advances) = super::source_set_function::probe::run(None, || {
            cassie.execute_sql_with_controls(
                &session,
                sql,
                vec![],
                ExecutionMode::SimpleQuery,
                &controls,
            )
        });
        // Assert
        let result = result.expect("composed pull result");
        assert_eq!(result.rows(), &[vec![Value::Int64(2)]], "{sql}");
        assert_eq!(advances, 2, "{sql}");
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0, "{sql}");
    }
}

#[test]
fn should_skip_generated_source_invocation_given_zero_limit_at_ordinary_boundaries() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    let queries = [
        "SELECT * FROM generate_series(1,3,0) LIMIT 0",
        "SELECT * FROM (SELECT * FROM generate_series(1,3,0) LIMIT 0) s",
        "SELECT * FROM (VALUES(1)) v(k) CROSS JOIN LATERAL generate_series(1,3,0) s(n) LIMIT 0",
    ];

    for sql in queries {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        // Act
        let (result, advances) = super::source_set_function::probe::run(None, || {
            cassie.execute_sql_with_controls(
                &session,
                sql,
                vec![],
                ExecutionMode::SimpleQuery,
                &controls,
            )
        });
        // Assert
        let result = result.expect("zero-limit source remains unopened");
        assert!(result.rows().is_empty(), "{sql}");
        assert_eq!(advances, 0, "{sql}");
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0, "{sql}");
    }
}

#[test]
fn should_resolve_independent_subqueries_once_while_pulling_generated_rows() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    for name in ["source_filter_once", "source_projection_once"] {
        cassie
            .execute_sql(
                &session,
                &format!("CREATE SEQUENCE {name} START WITH 1"),
                vec![],
            )
            .expect("sequence fixture");
    }

    // Act
    let filtered = cassie.execute_sql(
        &session,
        "SELECT n FROM generate_series(1,3) s(n) WHERE (SELECT nextval('source_filter_once')) = 1",
        vec![],
    );
    let projected = cassie.execute_sql(&session,"SELECT token FROM (SELECT (SELECT nextval('source_projection_once')) AS token FROM generate_series(1,3) s(n)) d",vec![]);

    // Assert
    assert_eq!(
        filtered.expect("independent source predicate").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)]
        ]
    );
    assert_eq!(
        projected.expect("independent derived projection").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(1)],
            vec![Value::Int64(1)]
        ]
    );
    for name in ["source_filter_once", "source_projection_once"] {
        assert_eq!(
            cassie
                .execute_sql(&session, &format!("SELECT nextval('{name}')"), vec![])
                .expect("single evaluation witness")
                .rows(),
            &[vec![Value::Int64(2)]]
        );
    }
}

#[test]
fn should_stop_collection_input_after_a_filtered_lateral_source_satisfies_limit() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE lazy_source_input (id INT, items INT[])",
        "INSERT INTO lazy_source_input VALUES (1,ARRAY[1,2]),(2,ARRAY[1,2]),(3,ARRAY[1,2])",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("collection source fixture");
    }
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );

    // Act
    let (result, generated, collection_rows) = super::source_set_function::probe::run_all(
        None,
        || {
            cassie.execute_sql_with_controls(&session,
            "SELECT n FROM lazy_source_input t CROSS JOIN LATERAL unnest(t.items) s(n) WHERE n=2 LIMIT 1",
            vec![],ExecutionMode::SimpleQuery,&controls)
        },
    );

    // Assert
    let result = result.expect("lazy collection consumer");
    assert_eq!(result.rows(), &[vec![Value::Int64(2)]]);
    assert_eq!(generated, 2);
    assert_eq!(
        collection_rows, 1,
        "the consumer must not decode the other collection rows"
    );
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_each_selected_series_family_at_an_admitted_row_boundary() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    let queries = [
        "SELECT * FROM generate_series(1,5)",
        "SELECT * FROM generate_series(1::bigint,5::bigint)",
        "SELECT * FROM generate_series(1.00::numeric,5.00::numeric)",
        "SELECT * FROM generate_series(timestamp '2026-01-01',timestamp '2026-01-05',interval '1 day')",
        "SELECT * FROM generate_series(timestamptz '2026-01-01 00:00:00+00',timestamptz '2026-01-05 00:00:00+00',interval '1 day','UTC')",
    ];

    for sql in queries {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        // Act
        let (result, advances) =
            super::source_set_function::probe::run(Some((2, cancellation)), || {
                cassie.execute_sql_with_controls(
                    &session,
                    sql,
                    vec![],
                    ExecutionMode::SimpleQuery,
                    &controls,
                )
            });
        // Assert
        assert!(matches!(result, Err(CassieError::QueryCancelled)), "{sql}");
        assert_eq!(advances, 2, "{sql}");
        assert_eq!(controls.current_query_memory_bytes(), 0, "{sql}");
        assert_eq!(
            cassie
                .execute_sql(&session, "SELECT 1", vec![])
                .expect("recovery")
                .rows(),
            &[vec![Value::Int64(1)]]
        );
    }
}

#[test]
fn should_keep_collection_row_owners_live_until_the_last_retained_row_is_dropped() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE source_row_owners (id INT, label TEXT)",
        "INSERT INTO source_row_owners VALUES (1,'first'),(2,'second')",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("controlled row fixture");
    }
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let mut stream = crate::executor::scan::projected_scan_stream(
        &cassie,
        Some(&session),
        "source_row_owners",
        &["id".to_owned(), "label".to_owned()],
        None,
        &controls,
    )
    .expect("controlled source")
    .expect("schema-backed transport");

    // Act
    let first = stream
        .next_retained_row()
        .expect("first conversion")
        .expect("first row");
    let second = stream
        .next_retained_row()
        .expect("second conversion")
        .expect("second row");
    drop(stream);

    // Assert
    assert!(first.query_memory().is_some());
    assert!(second.query_memory().is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(first);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(second);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_apply_offset_after_each_volatile_projection_without_overrunning_limit() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    let cases = [
        ("source_offset_direct","SELECT nextval('source_offset_direct') FROM generate_series(1,3) OFFSET 1 LIMIT 1"),
        ("source_offset_outer","SELECT token FROM (SELECT nextval('source_offset_outer') AS token FROM generate_series(1,3)) d OFFSET 1 LIMIT 1"),
        ("source_offset_inner","SELECT token FROM (SELECT nextval('source_offset_inner') AS token FROM generate_series(1,3) OFFSET 1 LIMIT 1) d"),
    ];
    for (name, _) in cases {
        cassie
            .execute_sql(
                &session,
                &format!("CREATE SEQUENCE {name} START WITH 1"),
                vec![],
            )
            .expect("projection sequence");
    }

    for (name, sql) in cases {
        // Act
        assert_eq!(
            cassie
                .execute_sql(&session, sql, vec![])
                // Assert
                .expect("offset projection")
                .rows(),
            &[vec![Value::Int64(2)]],
            "{sql}"
        );
        // Observe the next sequence value after pagination.
        assert_eq!(
            cassie
                .execute_sql(&session, &format!("SELECT nextval('{name}')"), vec![])
                // Assert
                .expect("projection count witness")
                .rows(),
            &[vec![Value::Int64(3)]],
            "{sql}"
        );
    }
}

#[test]
fn should_apply_source_pagination_to_each_existing_expanded_projection_row() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    let sql =
        r#"SELECT json_object_keys('{"a":1,"b":2}') FROM generate_series(1,1) OFFSET 1 LIMIT 1"#;

    // Act
    let result = cassie.execute_sql(&session, sql, vec![]);

    // Assert
    assert_eq!(
        result.expect("existing key projection pagination").rows(),
        &[vec![Value::String("b".to_owned())]]
    );
}

#[test]
fn should_preserve_object_key_expansion_in_outer_and_nested_pull_cursors() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    for (sql, expected) in [
        (
            r#"SELECT json_object_keys('{"a":1,"b":2}') AS k FROM generate_series(1,2) OFFSET 1 LIMIT 2"#,
            vec![
                vec![Value::String("b".into())],
                vec![Value::String("a".into())],
            ],
        ),
        (
            r#"SELECT d.k FROM (SELECT json_object_keys('{"a":1,"b":2}') AS k FROM generate_series(1,2)) d OFFSET 1 LIMIT 2"#,
            vec![
                vec![Value::String("b".into())],
                vec![Value::String("a".into())],
            ],
        ),
        (
            r#"SELECT json_object_keys('{}') AS k FROM generate_series(1,2)"#,
            Vec::new(),
        ),
    ] {
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        // Act
        let result = cassie
            .execute_sql_with_controls(&session, sql, vec![], ExecutionMode::SimpleQuery, &controls)
            .expect("pull result");
        // Assert
        assert_eq!(result.rows(), expected, "{sql}");
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0, "{sql}");
    }
}

#[test]
fn should_release_pull_owners_when_cancelled_during_actual_object_key_pull_move() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    for sql in [
        r#"SELECT json_object_keys('{"a":1,"b":2}') AS k FROM generate_series(1,2)"#,
        r#"SELECT d.k FROM (SELECT json_object_keys('{"a":1,"b":2}') AS k FROM generate_series(1,2)) d"#,
    ] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        crate::executor::projection::cancel_object_key_expansion_once(cancel.clone(), 10);
        // Act
        let result = cassie.execute_sql_with_controls(
            &session,
            sql,
            vec![],
            ExecutionMode::SimpleQuery,
            &controls,
        );
        // Assert
        assert!(cancel.is_cancelled(), "{sql}");
        assert!(result.is_err(), "{sql}");
        assert_eq!(controls.current_query_memory_bytes(), 0, "{sql}");
    }
}

#[test]
fn should_release_pull_resources_when_object_key_projection_fails() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    // Act
    let result = cassie.execute_sql_with_controls(
        &session,
        "SELECT json_object_keys('1') FROM generate_series(1,2)",
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_pull_expansion_peak_and_release_on_failure() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    let sql = r#"SELECT json_object_keys('{"a":1,"b":2}') AS k FROM generate_series(1,2) OFFSET 1 LIMIT 2"#;
    let generous = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let result = cassie
        .execute_sql_with_controls(&session, sql, vec![], ExecutionMode::SimpleQuery, &generous)
        .expect("baseline");
    let peak = generous.peak_query_memory_bytes();
    drop(result);
    assert_eq!(generous.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                query_timeout_ms: 0,
                ..crate::config::CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let result = cassie.execute_sql_with_controls(
            &session,
            sql,
            vec![],
            ExecutionMode::SimpleQuery,
            &controls,
        );
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
