use super::*;
use crate::types::DataType;

#[test]
fn should_preserve_bound_conditional_float_values_through_typed_scalar_projection() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-typed-conditional-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE conditional_transport (n BIGINT, f FLOAT, flag BOOLEAN)",
        "INSERT INTO conditional_transport VALUES (9007199254740993, NULL, TRUE), (1, NULL, FALSE)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let statement = crate::sql::parse_statement(
        "SELECT CAST(NULLIF(n, f) AS FLOAT) AS a, CAST(GREATEST(COALESCE(n, f), f) AS FLOAT) AS b, CAST(LEAST(n, f) AS FLOAT) AS c FROM conditional_transport WHERE flag OR n > 9007199254740992",
    )
    .expect("statement");
    let bound = crate::sql::binder::bind(statement, &cassie.catalog).expect("initial binding");
    let plan = crate::planner::logical::plan(&bound).expect("plan");
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    let batch = TypedBatch::from_columns(
        &controls,
        &[
            ("n".to_owned(), DataType::BigInt),
            ("f".to_owned(), DataType::Float),
            ("flag".to_owned(), DataType::Boolean),
        ],
        &[
            vec![Value::Int64(9_007_199_254_740_993), Value::Int64(1)],
            vec![Value::Null, Value::Null],
            vec![Value::Bool(true), Value::Bool(false)],
        ],
        2,
        None,
    )
    .expect("typed input");

    // Act
    let route = try_execute(
        &cassie,
        Some(&session),
        &plan,
        &HashMap::new(),
        &[],
        &controls,
    )
    .expect("route selection");
    let filtered = batch
        .filter(
            &controls,
            plan.filter.as_ref().expect("native predicate"),
            &[],
            &HashMap::new(),
            Some(&session),
        )
        .expect("native filter");
    let projected = filtered
        .project(
            &controls,
            &plan.projection,
            &[],
            &HashMap::new(),
            Some(&session),
        )
        .expect("bounded scalar projection");
    let rows = output_rows(&projected, &controls).expect("output handoff");

    // Assert
    assert!(route.is_none(), "function queries retain scalar dispatch");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].entries(),
        &[
            ("a".to_owned(), Value::Float64(9_007_199_254_740_992.0)),
            ("b".to_owned(), Value::Float64(9_007_199_254_740_992.0)),
            ("c".to_owned(), Value::Float64(9_007_199_254_740_992.0)),
        ]
    );
    assert_eq!(
        rows[0].data_types(),
        &[DataType::Float, DataType::Float, DataType::Float]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(rows);
    drop((projected, filtered, batch));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}
