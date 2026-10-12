use super::{execute_scalar_index_read, Cassie, Expr, QueryExecutionControls};
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use crate::sql::ast::BinaryOp;
use crate::types::{DataType, Value};
use std::collections::HashMap;
use std::time::Instant;

fn generated_float(expression: Expr) -> Expr {
    Expr::Cast {
        expr: Box::new(Expr::Case {
            operand: None,
            branches: Vec::new(),
            else_expr: Some(Box::new(expression)),
        }),
        data_type: DataType::Float,
    }
}

#[test]
fn should_decline_integer_index_for_manually_wrapped_float_comparison() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-manual-float-index-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("fixture");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE manual_float_index (k BIGINT)",
        "INSERT INTO manual_float_index (k) VALUES (9007199254740992), (9007199254740993)",
        "CREATE INDEX manual_float_index_k ON manual_float_index USING btree (k)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("seed");
    }
    let index = cassie
        .catalog
        .all_indexes_snapshot()
        .into_iter()
        .find(|index| index.name.ends_with("manual_float_index_k"))
        .expect("real index");
    let parsed =
        crate::sql::parse_statement(&format!("SELECT k FROM {}", index.collection)).expect("query");
    let mut logical = super::super::build_logical_plan(&cassie.catalog, &parsed)
        .expect("logical plan");
    logical.filter = Some(Expr::Binary {
        left: Box::new(generated_float(Expr::Column("k".into()))),
        op: BinaryOp::Eq,
        right: Box::new(generated_float(Expr::IntegerLiteral(9_007_199_254_740_993))),
    });
    let physical = crate::planner::physical::build_with_indexes(
        logical.clone(),
        &[index],
        &HashMap::<String, crate::catalog::CollectionCardinalityStats>::new(),
    );
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());

    // Act
    let indexed = execute_scalar_index_read(
        &cassie,
        None,
        Some(&physical),
        &logical,
        &HashMap::new(),
        &[],
        &controls,
    )
    .expect("safe admission decision");
    let result = super::super::entrypoints::run_with_controls(
        &cassie,
        &std::sync::Arc::new(physical),
        vec![],
        &controls,
    )
    .expect("authoritative fallback");
    let rows = result.rows().to_vec();
    let live = controls.current_query_memory_bytes();
    drop(result);
    let retired = controls.current_query_memory_bytes();
    drop(session);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("retire fixture");

    // Assert
    assert!(
        indexed.is_none(),
        "a public AST convention is not a catalog type proof"
    );
    let mut integers = rows
        .into_iter()
        .map(|row| row[0].clone())
        .collect::<Vec<_>>();
    integers.sort_by_key(|value| match value {
        Value::Int64(integer) => *integer,
        _ => panic!("original integer"),
    });
    assert_eq!(
        integers,
        vec![
            Value::Int64(9_007_199_254_740_992),
            Value::Int64(9_007_199_254_740_993)
        ]
    );
    assert!(live > 0);
    assert_eq!(retired, 0);
}
