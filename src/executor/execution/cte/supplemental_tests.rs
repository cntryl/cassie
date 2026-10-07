use super::*;
use std::time::{Duration, Instant};

fn fixture(run: impl FnOnce(&Cassie)) {
    let path = std::env::temp_dir().join(format!("cassie-857-controls-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    run(&cassie);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict cleanup");
}

fn run_sql(
    cassie: &Cassie,
    sql: &str,
    controls: &QueryExecutionControls,
) -> (Result<Vec<BatchRow>, QueryError>, CteContext) {
    let statement = crate::sql::parse_statement(sql).expect("supported SQL");
    let plan =
        super::super::build_logical_plan_in_session(cassie, None, &statement).expect("bound plan");
    let mut context = CteContext::new();
    let output = execute_plan(
        cassie,
        None,
        &plan,
        &mut context,
        &HashMap::new(),
        &[],
        controls,
    );
    (output, context)
}

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: budget,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_preserve_multiple_cte_references_and_deep_context_independence() {
    // Arrange
    fixture(|cassie| {
        let controls = controls(1024 * 1024);
        let (output, mut context) = run_sql(cassie, "WITH c AS (SELECT CAST(7 AS BIGINT) AS n), d AS (SELECT n FROM c) SELECT c.n FROM c JOIN d ON c.n = d.n", &controls);
        let output = output.expect("multiple references");
        let before = controls.current_query_memory_bytes();
        // Act
        let copy = context.copy(&controls).expect("independent context copy");
        let after = controls.current_query_memory_bytes();
        let mut removed = context.remove("c").expect("original relation");
        removed.rows[0][0].1 = Value::Int64(99);
        drop(removed);
        // Assert
        assert!(after > before);
        assert_eq!(copy["c"].rows[0][0].1, Value::Int64(7));
        assert_eq!(output[0].entries()[0].1, Value::Int64(7));
        drop(context);
        drop(output);
        assert!(controls.current_query_memory_bytes() > 0);
        drop(copy);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_preserve_derived_lateral_and_exists_cte_copy_paths() {
    // Arrange
    fixture(|cassie| {
        for sql in [
            "WITH c AS (SELECT CAST(7 AS BIGINT) AS n) SELECT q.n FROM (SELECT n FROM c) AS q",
            "WITH c AS (SELECT CAST(7 AS BIGINT) AS n) SELECT c.n FROM c JOIN LATERAL (SELECT n FROM c) AS q ON true",
            "WITH c AS (SELECT CAST(7 AS BIGINT) AS n) SELECT n FROM c WHERE EXISTS (SELECT n FROM c)",
        ] {
            let controls = controls(1024 * 1024);
            // Act
            let (output, context) = run_sql(cassie, sql, &controls);
            let output = output.expect(sql);
            // Assert
            assert_eq!(output.len(), 1, "{sql}");
            assert_eq!(output[0].entries()[0].1, Value::Int64(7), "{sql}");
            drop(context);
            assert!(controls.current_query_memory_bytes() > 0, "{sql}");
            drop(output);
            assert_eq!(controls.current_query_memory_bytes(), 0, "{sql}");
        }
    });
}

#[test]
fn should_preserve_recursive_union_all_empty_and_depth_failure_cleanup() {
    // Arrange
    fixture(|cassie| {
        for operator in ["UNION", "UNION ALL"] {
            let controls = controls(1024 * 1024);
            let sql = format!("WITH RECURSIVE seq(n) AS (SELECT CAST(0 AS INT) {operator} SELECT CAST(n + 1 AS INT) FROM seq WHERE n < 3) SELECT n FROM seq ORDER BY n");
            // Act
            let (output, context) = run_sql(cassie, &sql, &controls);
            let output = output.expect("finite recursion");
            // Assert
            assert_eq!(
                output
                    .iter()
                    .map(|row| row.entries()[0].1.clone())
                    .collect::<Vec<_>>(),
                vec![
                    Value::Int64(0),
                    Value::Int64(1),
                    Value::Int64(2),
                    Value::Int64(3)
                ]
            );
            assert!(controls.peak_query_memory_bytes() > controls.current_query_memory_bytes());
            drop(output);
            drop(context);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
        let controls = controls(1024 * 1024);
        let (output, context) = run_sql(cassie, "WITH RECURSIVE seq(n) AS (SELECT CAST(0 AS INT) WHERE false UNION ALL SELECT CAST(n + 1 AS INT) FROM seq WHERE n < 3) SELECT n FROM seq", &controls);
        assert!(output.expect("empty recursive seed").is_empty());
        drop(context);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        let limits = crate::config::CassieRuntimeLimits {
            cte_recursion_depth: 2,
            query_timeout_ms: 0,
            ..crate::config::CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let (output, context) = run_sql(cassie, "WITH RECURSIVE seq(n) AS (SELECT CAST(0 AS INT) UNION ALL SELECT CAST(n + 1 AS INT) FROM seq) SELECT n FROM seq", &controls);
        assert!(output
            .expect_err("depth bound")
            .to_string()
            .contains("maximum recursion depth"));
        drop(context);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_release_context_and_source_copy_attempts_on_denial_cancel_and_deadline() {
    // Arrange
    let row = vec![("n".into(), Value::String("x".repeat(4096)))];
    let relation = CteRelation {
        rows: vec![row],
        fields: vec![],
        _rows_memory: None,
        _fields_memory: None,
    };
    let denied = controls(128);
    // Act
    let result = relation.copy(&denied);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    let handle = crate::runtime::QueryCancellationHandle::new();
    let cancelled = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        handle.clone(),
    );
    handle.cancel();
    assert!(matches!(
        relation
            .copy(&cancelled)
            .map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(cancelled.current_query_memory_bytes(), 0);
    let expired = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_timeout_ms: 1,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now() - Duration::from_secs(1),
    );
    assert!(matches!(
        relation
            .copy(&expired)
            .map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::DeadlineExceeded)
    ));
    assert_eq!(expired.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_rich_source_copies_and_deny_old_new_peak_minus_one() {
    // Arrange
    let rows = vec![vec![
        ("text".into(), Value::String("\n".repeat(4096))),
        (
            "vector".into(),
            Value::Vector(crate::types::Vector::new(vec![1.0; 1024])),
        ),
        (
            "json".into(),
            Value::Json(
                serde_json::json!({"escaped": "\n".repeat(4096), "array": [1, null, true]}),
            ),
        ),
    ]];
    let fields = vec![crate::types::FieldSchema {
        name: "json".into(),
        data_type: crate::types::DataType::Array(Box::new(crate::types::DataType::Json)),
        nullable: false,
    }];
    let run = |budget| {
        let controls = controls(budget);
        let retained = RetainedRows::copy(&rows, &controls).expect("original materialization");
        let fields_memory = controls
            .reserve_query_memory(retention::fields_bytes(&fields).expect("fields bound"))
            .expect("original fields");
        let relation = CteRelation {
            rows: retained.rows,
            fields: fields.clone(),
            _rows_memory: Some(retained.memory),
            _fields_memory: Some(fields_memory),
        };
        let before = controls.current_query_memory_bytes();
        let first = copy_source_rows(&relation, &controls, Some("c"));
        (controls, relation, before, first)
    };
    // Act
    let (successful, relation, before, first) = run(2 * 1024 * 1024);
    let first = first.expect("first independent copy");
    let peak = successful.peak_query_memory_bytes();
    let second = copy_source_rows(&relation, &successful, Some("c")).expect("second reference");
    // Assert
    assert!(successful.current_query_memory_bytes() > before);
    assert_eq!(first[0].entries(), rows[0]);
    assert_eq!(second[0].entries(), rows[0]);
    assert_eq!(relation.fields[0].data_type, fields[0].data_type);
    drop(relation);
    drop(first);
    assert!(successful.current_query_memory_bytes() > 0);
    drop(second);
    assert_eq!(successful.current_query_memory_bytes(), 0);
    let (denied, original, original_charge, result) = run(peak - 1);
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(original.rows, rows);
    assert_eq!(denied.current_query_memory_bytes(), original_charge);
    drop(original);
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_original_context_on_deep_copy_budget_denial() {
    // Arrange
    let controls = controls(32 * 1024);
    let retained = RetainedRows::copy(
        &vec![vec![("n".into(), Value::String("x".repeat(4096)))]],
        &controls,
    )
    .expect("original");
    let relation = CteRelation {
        rows: retained.rows,
        fields: vec![],
        _rows_memory: Some(retained.memory),
        _fields_memory: None,
    };
    let mut context = CteContext::new();
    context.insert("c", relation, &controls).expect("context");
    let before = controls.current_query_memory_bytes();
    let external = controls
        .reserve_query_memory(32 * 1024 - before - 1)
        .expect("external live parent");
    // Act
    let result = context.copy(&controls);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(context["c"].rows[0][0].1, Value::String("x".repeat(4096)));
    drop(external);
    assert_eq!(controls.current_query_memory_bytes(), before);
    drop(context);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_recursive_growth_state_on_resource_denial() {
    // Arrange
    fixture(|cassie| {
        let controls = controls(1024);
        // Act
        let (output, context) = run_sql(cassie, "WITH RECURSIVE seq(n) AS (SELECT CAST(0 AS INT) UNION ALL SELECT CAST(n + 1 AS INT) FROM seq WHERE n < 10) SELECT n FROM seq", &controls);
        // Assert
        assert!(matches!(
            output.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::ResourceLimit(_))
        ));
        drop(context);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_release_recursive_delta_copy_after_mid_copy_cancellation() {
    // Arrange
    fixture(|cassie| {
        let handle = crate::runtime::QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
            handle.clone(),
        );
        let parent = controls
            .reserve_query_memory(65_536)
            .expect("external owner");
        retention_tests::arm_copy_cancellation(handle);
        // Act
        let (output, context) = run_sql(cassie, "WITH RECURSIVE seq(n) AS (SELECT CAST(0 AS INT) UNION ALL SELECT CAST(n + 1 AS INT) FROM seq WHERE n < 3) SELECT n FROM seq", &controls);
        // Assert
        assert!(matches!(
            output.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::QueryCancelled)
        ));
        drop(context);
        assert_eq!(controls.current_query_memory_bytes(), 65_536);
        drop(parent);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_deny_accumulated_recursive_growth_before_mutating_original_rows() {
    // Arrange
    let controls = controls(32 * 1024);
    let mut original = RetainedRows::copy(
        &vec![vec![("n".into(), Value::String("x".repeat(4096)))]],
        &controls,
    )
    .expect("original state");
    let next = RetainedRows::copy(
        &vec![vec![("n".into(), Value::String("y".repeat(4096)))]],
        &controls,
    )
    .expect("next delta");
    let before = controls.current_query_memory_bytes();
    let fill = controls
        .reserve_query_memory(32 * 1024 - before - 1)
        .expect("external live owner");
    // Act
    let result = original.append(&next.rows, &controls);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(original.rows.len(), 1);
    assert_eq!(original.rows[0][0].1, Value::String("x".repeat(4096)));
    drop(fill);
    assert_eq!(controls.current_query_memory_bytes(), before);
    drop(next);
    drop(original);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_actual_recursive_seen_table_and_rich_key_capacities() {
    // Arrange
    let controls = controls(1024 * 1024);
    let mut seen = seen::SeenRows::new(&controls).expect("seen state");
    let mut capacities = Vec::new();
    // Act
    for index in 0..8 {
        let row = vec![
            ("identity".into(), Value::Int64(index)),
            (
                "timestamp".into(),
                Value::String("2024-01-01T12:00:00Z".into()),
            ),
            (
                "json".into(),
                Value::Json(
                    serde_json::json!({"escaped": "\n".repeat([17, 33, 65, 129][usize::try_from(index).expect("index") % 4])}),
                ),
            ),
            (
                "vector".into(),
                Value::Vector(crate::types::Vector::new(vec![1.0, 2.0, 3.0])),
            ),
        ];
        assert!(seen.insert(&row, &controls).expect("unique key"));
        let (capacity, admitted, actual) = seen.capacity_probe();
        capacities.push(capacity);
        println!(
            "seen length{} capacity{capacity} admitted{admitted} actual{actual}",
            index + 1
        );
        // Assert
        assert!(
            admitted >= actual,
            "actual retained key/table capacity must be bounded"
        );
        let before = controls.current_query_memory_bytes();
        assert!(!seen.insert(&row, &controls).expect("duplicate key"));
        assert_eq!(controls.current_query_memory_bytes(), before);
    }
    assert!(capacities[0] >= 1 && capacities[2] >= 3 && capacities[3] >= 4 && capacities[7] >= 8);
    drop(seen);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_non_power_of_two_accumulator_capacity_and_partial_append_cancellation() {
    // Arrange
    let handle = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        handle.clone(),
    );
    let rich_row = |index| {
        vec![
            (
                "n".into(),
                Value::Json(serde_json::json!({"index": index, "text": "\n".repeat(129)})),
            ),
            (
                "v".into(),
                Value::Vector(crate::types::Vector::new(vec![1.0; 3])),
            ),
        ]
    };
    let initial = (0..3).map(rich_row).collect::<Vec<_>>();
    let mut original = RetainedRows::copy(&initial, &controls).expect("original");
    assert_eq!(original.rows.capacity(), 3);
    let delta = RetainedRows::copy(&(3..5).map(rich_row).collect(), &controls).expect("delta");
    // Act
    original
        .append(&delta.rows, &controls)
        .expect("nonpower growth");
    // Assert
    assert_eq!(original.rows.capacity(), 5);
    assert!(
        original.memory.bytes()
            >= retention::rows_bytes(&original.rows).expect("actual original capacity")
    );
    assert!(
        controls.current_query_memory_bytes()
            >= retention::rows_bytes(&original.rows).expect("actual original")
                + retention::rows_bytes(&delta.rows).expect("actual delta")
    );
    retention_tests::arm_copy_cancellation(handle);
    let result = original.append(&delta.rows, &controls);
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(
        original.rows.len(),
        6,
        "one actual append occurs before cancellation"
    );
    assert!(
        original.memory.bytes()
            >= retention::rows_bytes(&original.rows).expect("partial retained capacity")
    );
    assert!(
        controls.current_query_memory_bytes()
            >= retention::rows_bytes(&original.rows).expect("partial original")
                + retention::rows_bytes(&delta.rows).expect("live delta")
    );
    drop(original);
    drop(delta);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
