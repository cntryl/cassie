use super::*;
use crate::config::CassieRuntimeLimits;
use crate::types::DataType;
use std::time::Instant;

#[test]
fn should_fold_selected_typed_integer_lanes_without_losing_precision() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let batch = TypedBatch::from_columns(
        &controls,
        &[("n".to_owned(), DataType::BigInt)],
        &[vec![
            Value::Int64(9_007_199_254_740_993),
            Value::Null,
            Value::Int64(-9_007_199_254_740_992),
        ]],
        3,
        Some(&[1, 0, 2]),
    )
    .expect("typed source");
    // Act
    let sum = fold_sum(&batch, &controls).expect("typed sum");
    // Assert
    assert_eq!(sum, Value::Int64(1));
    drop(batch);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn sum_at_split(values: &[Value], data_type: &DataType, split: usize) -> Result<Value, QueryError> {
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut state = State::new("sum").expect("SUM");
    for values in [&values[..split], &values[split..]] {
        let batch = TypedBatch::from_columns(
            &controls,
            &[("n".to_owned(), data_type.clone())],
            &[values.to_vec()],
            values.len(),
            None,
        )?;
        consume(&mut state, Some(0), &batch, &controls)?;
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
    state.finish()
}

#[test]
fn should_retain_integer_prefix_overflow_across_every_typed_batch_split() {
    // Arrange
    let overflow = [Value::Int64(i64::MAX), Value::Int64(1), Value::Int64(-1)];
    let valid = [
        Value::Int64(-i64::MAX),
        Value::Int64(i64::MAX),
        Value::Int64(i64::MAX),
    ];
    // Act
    for split in 0..=overflow.len() {
        let failure =
            sum_at_split(&overflow, &DataType::BigInt, split).expect_err("prefix overflow");
        let sum = sum_at_split(&valid, &DataType::BigInt, split).expect("global row-order fold");
        // Assert
        assert!(failure.to_string().contains("aggregate integer overflow"));
        assert_eq!(sum, Value::Int64(i64::MAX));
    }
}

#[test]
fn should_preserve_float_fold_semantics_across_typed_splits() {
    // Arrange
    let ordered = [
        Value::Float64(1e16),
        Value::Float64(1.0),
        Value::Float64(-1e16),
    ];
    let overflow = [
        Value::Float64(f64::MAX),
        Value::Float64(f64::MAX),
        Value::Float64(-f64::MAX),
    ];
    // Act
    for split in 0..=ordered.len() {
        let sum = sum_at_split(&ordered, &DataType::Float, split).expect("row-order sum");
        let failure = sum_at_split(&overflow, &DataType::Float, split).expect_err("float overflow");
        // Assert
        assert_eq!(sum, Value::Float64(0.0));
        assert!(failure.to_string().contains("overflow"));
    }
}

#[test]
fn should_preserve_typed_aggregate_identities_without_non_null_inputs() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    for values in [vec![], vec![Value::Null, Value::Null]] {
        let batch = TypedBatch::from_columns(
            &controls,
            &[("n".to_owned(), DataType::BigInt)],
            &[values.clone()],
            values.len(),
            None,
        )
        .expect("typed null source");
        for name in ["count", "sum", "avg", "min", "max"] {
            let mut state = State::new(name).expect("state");
            consume(&mut state, Some(0), &batch, &controls).expect("fold");
            // Assert
            let expected = if name == "count" {
                Value::Int64(0)
            } else {
                Value::Null
            };
            assert_eq!(state.finish().expect("finish"), expected);
        }
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_numeric_aggregate_stream_owners() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let path =
        std::env::temp_dir().join(format!("cassie-typed-aggregate-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE numbers (n BIGINT)",
        "INSERT INTO numbers VALUES (9007199254740993), (-9007199254740992), (NULL)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let statement = crate::sql::parse_statement("SELECT COUNT(*) AS rows, COUNT(n) AS nonnull, SUM(n) AS total, AVG(n) AS mean, MIN(n) AS low, MAX(n) AS high FROM numbers").expect("statement");
    let plan =
        super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
            .expect("plan");
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    // Act
    let output = try_execute(&cassie, Some(&session), &plan, &controls)
        .expect("execution")
        .expect("native aggregate path");
    // Assert
    assert_eq!(output.len(), 1);
    assert_eq!(
        output[0].entries(),
        &[
            ("rows".to_owned(), Value::Int64(3)),
            ("nonnull".to_owned(), Value::Int64(2)),
            ("total".to_owned(), Value::Int64(1)),
            ("mean".to_owned(), Value::Float64(0.0)),
            ("low".to_owned(), Value::Int64(-9_007_199_254_740_992)),
            ("high".to_owned(), Value::Int64(9_007_199_254_740_993))
        ]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_match_scalar_aggregate_semantics_for_selected_numeric_carriers() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let statement = crate::sql::parse_statement("SELECT SUM(n) FROM numbers").expect("statement");
    let plan = super::super::super::build_logical_plan(&crate::catalog::Catalog::new(), &statement)
        .expect("plan");
    let functions = std::collections::HashMap::new();
    let context = super::super::AggregateExecutionContext {
        plan: &plan,
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    // Act
    for (data_type, values) in numeric_cases() {
        let batch = TypedBatch::from_columns(
            &controls,
            &[("n".to_owned(), data_type)],
            &[values.clone()],
            values.len(),
            None,
        )
        .expect("typed source");
        let rows = values
            .into_iter()
            .map(|value| BatchRow::new(vec![("n".to_owned(), value)]))
            .collect::<Vec<_>>();
        for name in ["count", "sum", "avg", "min", "max"] {
            let function = crate::sql::ast::FunctionCall {
                name: name.to_owned(),
                args: vec![Expr::Column("n".to_owned())],
            };
            let scalar =
                super::super::state::AggregateAccumulator::evaluate(&function, &rows, &context);
            let mut state = State::new(name).expect("state");
            consume(&mut state, Some(0), &batch, &controls).expect("consume");
            let typed = state.finish();
            // Assert
            match (typed, scalar) {
                (Ok(Value::Float64(typed)), Ok(Value::Float64(scalar))) => {
                    assert_eq!(typed.to_bits(), scalar.to_bits(), "{name}")
                }
                (Ok(typed), Ok(scalar)) => assert_eq!(typed, scalar, "{name}"),
                (Err(typed), Err(scalar)) => {
                    assert_eq!(typed.to_string(), scalar.to_string(), "{name}")
                }
                (typed, scalar) => panic!("{name}: typed {typed:?}, scalar {scalar:?}"),
            }
        }
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_typed_aggregate_consumption_without_retaining_output() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let batch = TypedBatch::from_columns(
        &controls,
        &[("n".to_owned(), DataType::BigInt)],
        &[vec![Value::Int64(1)]],
        1,
        None,
    )
    .expect("batch");
    let mut state = State::new("sum").expect("state");
    cancellation.cancel();
    // Act
    let result = consume(&mut state, Some(0), &batch, &controls);
    // Assert
    assert!(result.is_err());
    drop(batch);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn numeric_cases() -> [(DataType, Vec<Value>); 9] {
    [
        (
            DataType::BigInt,
            vec![Value::Int64(i64::MAX), Value::Int64(1), Value::Int64(-1)],
        ),
        (
            DataType::BigInt,
            vec![
                Value::Int64(-i64::MAX),
                Value::Int64(i64::MAX),
                Value::Int64(i64::MAX),
            ],
        ),
        (
            DataType::BigInt,
            vec![
                Value::Int64(9_007_199_254_740_993),
                Value::Null,
                Value::Int64(-9_007_199_254_740_992),
            ],
        ),
        (
            DataType::SmallInt,
            vec![Value::Int64(12), Value::Int64(-7), Value::Null],
        ),
        (
            DataType::Int,
            vec![Value::Int64(2_147_483_647), Value::Int64(-2_147_483_648)],
        ),
        (
            DataType::Float,
            vec![Value::Float64(-0.0), Value::Float64(0.0)],
        ),
        (
            DataType::Float,
            vec![Value::Float64(f64::NAN), Value::Float64(1.0)],
        ),
        (
            DataType::Float,
            vec![
                Value::Float64(1e16),
                Value::Float64(1.0),
                Value::Float64(-1e16),
            ],
        ),
        (
            DataType::Float,
            vec![
                Value::Float64(f64::MAX),
                Value::Float64(f64::MAX),
                Value::Float64(-f64::MAX),
            ],
        ),
    ]
}

#[test]
fn should_fold_selected_numeric_view_representations() {
    // Arrange
    use crate::executor::typed_batch::Column;
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    for (data_type, value) in [
        (DataType::BigInt, Value::Int64(7)),
        (DataType::Float, Value::Float64(7.0)),
    ] {
        let parent = Column::constant(&controls, &data_type, &value, 5).expect("constant");
        let entries = Column::constant(&controls, &data_type, &value, 1).expect("entry");
        let mut views = vec![
            Column::constant(&controls, &data_type, &value, 3).expect("constant"),
            entries
                .dictionary(&controls, &[0, 0, 0], &[true; 3])
                .expect("dictionary"),
            parent.slice(&controls, 1, 3).expect("slice"),
            parent.gather(&controls, &[4, 0, 2]).expect("gather"),
        ];
        if data_type == DataType::BigInt {
            views.push(Column::sequence(&controls, &data_type, 7, 0, 3).expect("sequence"));
        }
        drop((parent, entries));
        // Act
        for view in views {
            let batch = TypedBatch::from_views(
                &controls,
                &[("n".to_owned(), data_type.clone())],
                &[view],
                3,
                Some(&[2, 0, 2, 1]),
            )
            .expect("repeated selected view");
            for name in ["count", "sum", "avg", "min", "max"] {
                let mut state = State::new(name).expect("state");
                consume(&mut state, Some(0), &batch, &controls).expect("consume representation");
                // Assert
                let expected = match name {
                    "count" => Value::Int64(4),
                    "avg" => Value::Float64(7.0),
                    "sum" if data_type == DataType::BigInt => Value::Int64(28),
                    "sum" => Value::Float64(28.0),
                    _ => value.clone(),
                };
                assert_eq!(state.finish().expect("finish"), expected, "{name}");
            }
        }
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn merged_integer_sum(values: &[i64], split: usize) -> Result<Value, QueryError> {
    let mut left = State::new("sum").expect("left SUM");
    let mut right = State::new("sum").expect("right SUM");
    for value in &values[..split] {
        left.update(Cell::Integer(*value))?;
    }
    for value in &values[split..] {
        right.update(Cell::Integer(*value))?;
    }
    left.merge_ordered(&right)?;
    left.finish()
}

#[test]
fn should_preserve_serial_prefix_overflow_when_merging_typed_worker_partials() {
    // Arrange
    let failures = [[i64::MAX, 1, -1], [i64::MIN, -1, 1]];
    let valid = [-i64::MAX, i64::MAX, i64::MAX];
    // Act
    for split in 0..=3 {
        for values in failures {
            let error = merged_integer_sum(&values, split).expect_err("serial prefix overflow");
            // Assert
            assert!(error.to_string().contains("aggregate integer overflow"));
        }
        assert_eq!(
            merged_integer_sum(&valid, split).expect("globally legal prefixes"),
            Value::Int64(i64::MAX)
        );
    }
}

#[test]
fn should_preserve_equal_carrier_order_in_typed_partial_merges() {
    // Arrange
    for (function, expected) in [
        ("count", Value::Int64(3)),
        ("min", Value::Int64(-7)),
        ("max", Value::Int64(9)),
    ] {
        let mut left = State::new(function).expect("left");
        let mut right = State::new(function).expect("right");
        left.update(Cell::Integer(9)).expect("left lane");
        right.update(Cell::Integer(-7)).expect("right lane");
        right.update(Cell::Integer(9)).expect("right equal lane");
        // Act
        left.merge_ordered(&right).expect("legal ordered merge");
        // Assert
        assert_eq!(left.finish().expect("finish"), expected);
    }
    let mut left = State::new("min").expect("left");
    let mut right = State::new("min").expect("right");
    left.update(Cell::Float(-0.0)).expect("negative zero");
    right.update(Cell::Float(0.0)).expect("positive zero");
    left.merge_ordered(&right).expect("ordered zero merge");
    let Value::Float64(value) = left.finish().expect("finish") else {
        panic!("FLOAT carrier")
    };
    assert!(value.is_sign_negative());
}

#[test]
fn should_publish_only_completed_bounded_typed_aggregation() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let rows = 2 * crate::executor::batch::DEFAULT_BATCH_SIZE + 3;
    for workers in [1, 2, 4] {
        let path =
            std::env::temp_dir().join(format!("cassie-typed-workers-{}", uuid::Uuid::new_v4()));
        let mut config = crate::config::CassieRuntimeConfig::from_env().expect("config");
        config.limits.parallel_aggregation_workers = workers;
        let cassie = Cassie::new_with_data_dir_and_config(path.to_str().expect("path"), config)
            .expect("Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE workers (n BIGINT)", vec![])
            .expect("table");
        cassie
            .midge
            .put_fresh_documents(
                "workers",
                (0..rows)
                    .map(|row| (Some(format!("{row:08}")), serde_json::json!({"n":row})))
                    .collect(),
            )
            .expect("seed");
        let statement = crate::sql::parse_statement(
            "SELECT COUNT(*) AS rows,SUM(n) AS total,MIN(n) AS low,MAX(n) AS high FROM workers",
        )
        .expect("statement");
        let plan =
            super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
                .expect("plan");
        let controls =
            QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
        let before = cassie.metrics();
        // Act
        let output = try_execute(&cassie, Some(&session), &plan, &controls)
            .expect("typed execution")
            .expect("native path");
        let after = cassie.metrics();
        // Assert
        let rows = i64::try_from(rows).expect("small fixture");
        assert_eq!(
            output[0].entries(),
            &[
                ("rows".into(), Value::Int64(rows)),
                ("total".into(), Value::Int64(rows * (rows - 1) / 2)),
                ("low".into(), Value::Int64(0)),
                ("high".into(), Value::Int64(rows - 1))
            ]
        );
        if workers > 1 {
            assert!(
                after["parallel_aggregation"]["aggregations"]
                    .as_u64()
                    .expect("completed aggregation counter")
                    > before["parallel_aggregation"]["aggregations"]
                        .as_u64()
                        .expect("baseline counter")
            );
        }
        assert_eq!(after["runtime"]["active_operator_workers"], 0);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop((session, cassie));
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}

#[test]
fn should_release_typed_worker_owners_without_success_diagnostics_after_prefix_overflow() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let path = std::env::temp_dir().join(format!(
        "cassie-typed-worker-overflow-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = crate::config::CassieRuntimeConfig::from_env().expect("config");
    config.limits.parallel_aggregation_workers = 2;
    let cassie =
        Cassie::new_with_data_dir_and_config(path.to_str().expect("path"), config).expect("Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE worker_overflow (n BIGINT)", vec![])
        .expect("table");
    let width = crate::executor::batch::DEFAULT_BATCH_SIZE;
    cassie
        .midge
        .put_fresh_documents(
            "worker_overflow",
            (0..2 * width)
                .map(|row| {
                    let value = if row == width - 1 {
                        i64::MAX
                    } else if row == width {
                        1
                    } else if row == width + 1 {
                        -1
                    } else {
                        0
                    };
                    (Some(format!("{row:08}")), serde_json::json!({"n":value}))
                })
                .collect(),
        )
        .expect("seed");
    let statement = crate::sql::parse_statement("SELECT SUM(n) AS total FROM worker_overflow")
        .expect("statement");
    let plan =
        super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
            .expect("plan");
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    let before = cassie.metrics();
    // Act
    let failure = try_execute(&cassie, Some(&session), &plan, &controls)
        .expect_err("serial prefix overflow survives worker merge");
    let after = cassie.metrics();
    // Assert
    assert!(failure.to_string().contains("aggregate integer overflow"));
    assert_eq!(
        after["parallel_aggregation"]["aggregations"],
        before["parallel_aggregation"]["aggregations"]
    );
    assert_eq!(
        after["parallel_aggregation"]["fallback_aggregations"],
        before["parallel_aggregation"]["fallback_aggregations"]
    );
    assert_eq!(after["runtime"]["active_operator_workers"], 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_handoff_encoded_numeric_predicates_to_typed_aggregates_without_objects() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let path =
        std::env::temp_dir().join(format!("cassie-typed-predicate-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("Cassie");
    let session = cassie.create_session("tester", None);
    for sql in ["CREATE TABLE predicate (n BIGINT, gate BIGINT)",
        "INSERT INTO predicate (n,gate) VALUES (9007199254740993,1),(-9007199254740992,1),(99,0),(NULL,1)",
        "CREATE INDEX predicate_idx ON predicate USING column (n,gate) WITH (segment_size = 2)"] {
        cassie.execute_sql(&session,sql,vec![]).expect("setup");
    }
    let sql = "SELECT COUNT(n) AS counted, SUM(n) AS total, MIN(n) AS low, MAX(n) AS high FROM predicate WHERE gate = 1 AND n IS NOT NULL";
    let statement = crate::sql::parse_statement(sql).expect("statement");
    let plan =
        super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
            .expect("plan");
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    let before = cassie.runtime.snapshot();
    // Act
    let output = try_execute(&cassie, Some(&session), &plan, &controls)
        .expect("typed execution")
        .expect("native predicate aggregate handoff");
    let after = cassie.runtime.snapshot();
    // Assert
    assert_eq!(
        output[0].entries(),
        &[
            ("counted".into(), Value::Int64(2)),
            ("total".into(), Value::Int64(1)),
            ("low".into(), Value::Int64(-9_007_199_254_740_992)),
            ("high".into(), Value::Int64(9_007_199_254_740_993))
        ]
    );
    assert!(after.column_batches.scans > before.column_batches.scans);
    assert_eq!(
        after.column_batches.materialized_values,
        before.column_batches.materialized_values
    );
    assert_eq!(
        after.aggregate_acceleration.direct_scans,
        before.aggregate_acceleration.direct_scans
    );
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let public = cassie
        .execute_sql(&session, sql, vec![])
        .expect("public typed predicate dispatch");
    assert_eq!(
        public.rows,
        vec![vec![
            Value::Int64(2),
            Value::Int64(1),
            Value::Int64(-9_007_199_254_740_992),
            Value::Int64(9_007_199_254_740_993)
        ]]
    );
    assert_eq!(public.columns[0].type_oid, 20);
    assert_eq!(
        cassie.runtime.snapshot().column_batches.materialized_values,
        after.column_batches.materialized_values
    );
    assert_eq!(
        cassie
            .runtime
            .snapshot()
            .aggregate_acceleration
            .direct_scans,
        after.aggregate_acceleration.direct_scans
    );
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_preserve_float_fold_semantics_through_encoded_numeric_owners() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    for workers in [1, 4] {
        let path =
            std::env::temp_dir().join(format!("cassie-typed-float-owner-{}", uuid::Uuid::new_v4()));
        let mut config = crate::config::CassieRuntimeConfig::from_env().expect("config");
        config.limits.parallel_aggregation_workers = workers;
        let cassie = Cassie::new_with_data_dir_and_config(path.to_str().expect("path"), config)
            .expect("Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE float_owner (n FLOAT)", vec![])
            .expect("table");
        let values = [
            Some(1e16),
            Some(1.0),
            Some(-1e16),
            Some(-0.0),
            Some(0.0),
            Some(1.5),
            None,
        ];
        cassie
            .midge
            .put_fresh_documents(
                "float_owner",
                values
                    .iter()
                    .enumerate()
                    .map(|(row, value)| (Some(format!("{row:08}")), serde_json::json!({"n":value})))
                    .collect(),
            )
            .expect("seed");
        let statement = crate::sql::parse_statement("SELECT SUM(n) AS total,AVG(n) AS mean,COUNT(n) AS counted,MIN(n) AS low,MAX(n) AS high FROM float_owner").expect("statement");
        let plan =
            super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
                .expect("plan");
        let controls =
            QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
        let row = try_execute(&cassie, Some(&session), &plan, &controls)
            .expect("row execution")
            .expect("typed row");
        let expected = row[0].entries().to_vec();
        drop(row);
        cassie.execute_sql(&session,"CREATE INDEX float_owner_idx ON float_owner USING column (n) WITH (segment_size = 2)",vec![]).expect("index");
        let before = cassie.runtime.snapshot();
        // Act
        let encoded = try_execute(&cassie, Some(&session), &plan, &controls)
            .expect("encoded execution")
            .expect("native owner");
        let after = cassie.runtime.snapshot();
        // Assert
        assert_eq!(encoded[0].entries(), expected);
        assert_eq!(encoded[0].get("total"), Some(&Value::Float64(1.5)));
        assert_eq!(encoded[0].get("mean"), Some(&Value::Float64(0.25)));
        assert_eq!(encoded[0].get("counted"), Some(&Value::Int64(6)));
        assert_eq!(
            after.column_batches.materialized_values,
            before.column_batches.materialized_values
        );
        assert_eq!(
            after.parallel_aggregation.aggregations,
            before.parallel_aggregation.aggregations
        );
        drop(encoded);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop((session, cassie));
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}

#[test]
fn should_cancel_after_a_completed_typed_worker_wave_without_success_or_retained_owners() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let path =
        std::env::temp_dir().join(format!("cassie-typed-wave-cancel-{}", uuid::Uuid::new_v4()));
    let mut config = crate::config::CassieRuntimeConfig::from_env().expect("config");
    config.limits.parallel_aggregation_workers = 2;
    let cassie =
        Cassie::new_with_data_dir_and_config(path.to_str().expect("path"), config).expect("Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE wave_cancel (n BIGINT)", vec![])
        .expect("table");
    let rows = 3 * crate::executor::batch::DEFAULT_BATCH_SIZE;
    cassie
        .midge
        .put_fresh_documents(
            "wave_cancel",
            (0..rows)
                .map(|row| (Some(format!("{row:08}")), serde_json::json!({"n":row})))
                .collect(),
        )
        .expect("seed");
    let statement =
        crate::sql::parse_statement("SELECT SUM(n) AS total,COUNT(*) AS counted FROM wave_cancel")
            .expect("statement");
    let plan =
        super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
            .expect("plan");
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &cassie.runtime.limits(),
        Instant::now(),
        cancellation.clone(),
    );
    let _probe = workers::WaveCancellationProbe::arm(cancellation);
    let before = cassie.runtime.snapshot();
    // Act
    let failure = try_execute(&cassie, Some(&session), &plan, &controls)
        .expect_err("cancel after two joined and ordered merged workers");
    let after = cassie.runtime.snapshot();
    // Assert
    assert!(failure.to_string().contains("cancel"));
    assert_eq!(
        after.parallel_aggregation.aggregations,
        before.parallel_aggregation.aggregations
    );
    assert_eq!(
        after.parallel_aggregation.fallback_aggregations,
        before.parallel_aggregation.fallback_aggregations
    );
    assert_eq!(after.runtime.active_operator_workers, 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}

fn fallback_fixture() -> (std::path::PathBuf, Cassie, CassieSession) {
    let path = std::env::temp_dir().join(format!("cassie-typed-fallback-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE fallback (n BIGINT,label TEXT)",
        "INSERT INTO fallback (n,label) VALUES (2,'keep'),(2,'omit'),(3,'keep'),(NULL,'keep')",
        "CREATE INDEX fallback_idx ON fallback USING column (n,label) WITH (segment_size = 2)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    (path, cassie, session)
}

#[test]
fn should_keep_excluded_aggregate_combinations_on_existing_fallback() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let (path, cassie, session) = fallback_fixture();
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    // Act
    for (sql, params, expected) in [
        (
            "SELECT DISTINCT COUNT(n) AS counted FROM fallback",
            vec![],
            Value::Int64(3),
        ),
        (
            "SELECT SUM(CASE WHEN n > 0 THEN n ELSE 0 END) AS total FROM fallback",
            vec![],
            Value::Int64(7),
        ),
        (
            "SELECT SUM(n) AS total FROM fallback WHERE label='keep'",
            vec![],
            Value::Int64(5),
        ),
        (
            "SELECT SUM(n) AS total FROM fallback WHERE n > $1",
            vec![Value::Int64(2)],
            Value::Int64(3),
        ),
    ] {
        let statement = crate::sql::parse_statement(sql).expect("statement");
        let plan =
            super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
                .expect("plan");
        let declined =
            try_execute(&cassie, Some(&session), &plan, &controls).expect("controlled decline");
        let result = cassie
            .execute_sql(&session, sql, params)
            .expect("existing fallback");
        // Assert
        assert!(declined.is_none(), "{sql}");
        assert_eq!(result.rows, vec![vec![expected]], "{sql}");
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let grouped_sql = "SELECT n,COUNT(*) AS counted FROM fallback GROUP BY n";
    let statement = crate::sql::parse_statement(grouped_sql).expect("statement");
    let plan =
        super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
            .expect("plan");
    assert!(try_execute(&cassie, Some(&session), &plan, &controls)
        .expect("group decline")
        .is_none());
    let grouped = cassie
        .execute_sql(&session, grouped_sql, vec![])
        .expect("grouped fallback");
    assert_eq!(grouped.rows.len(), 3);
    assert!(grouped
        .rows
        .contains(&vec![Value::Int64(2), Value::Int64(2)]));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_preserve_encoded_aggregate_identities_without_non_null_inputs() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let (path, cassie, session) = fallback_fixture();
    // Act
    for (predicate, expected) in [
        (
            "n IS NULL",
            vec![
                Value::Int64(1),
                Value::Int64(0),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            ],
        ),
        (
            "n > 99",
            vec![
                Value::Int64(0),
                Value::Int64(0),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            ],
        ),
    ] {
        let result = cassie.execute_sql(&session,&format!("SELECT COUNT(*),COUNT(n),SUM(n),AVG(n),MIN(n),MAX(n) FROM fallback WHERE {predicate}"),vec![]).expect("native empty/null handoff");
        // Assert
        assert_eq!(result.rows, vec![expected]);
        assert_eq!(result.columns[0].type_oid, 20);
        assert_eq!(result.columns[1].type_oid, 20);
    }
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_preserve_session_visibility_through_encoded_aggregation() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let (path, cassie, session) = fallback_fixture();
    // Act
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO fallback (n,label) VALUES (11,'keep')",
            vec![],
        )
        .expect("overlay");
    let before = cassie.runtime.snapshot();
    let own = cassie
        .execute_sql(
            &session,
            "SELECT SUM(n) FROM fallback WHERE n IS NOT NULL",
            vec![],
        )
        .expect("own overlay");
    let other = cassie.create_session("tester", None);
    let committed = cassie
        .execute_sql(
            &other,
            "SELECT SUM(n) FROM fallback WHERE n IS NOT NULL",
            vec![],
        )
        .expect("committed snapshot");
    // Assert
    assert_eq!(own.rows, vec![vec![Value::Int64(18)]]);
    assert_eq!(committed.rows, vec![vec![Value::Int64(7)]]);
    assert!(
        cassie.runtime.snapshot().column_batches.scans > before.column_batches.scans,
        "unmodified session can use encoded owner"
    );
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback");
    drop((other, session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}
