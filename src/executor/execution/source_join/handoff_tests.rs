use std::collections::HashMap;
use std::time::Instant;

use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryExecutionControls;

use super::{execute_query_source, BatchRow, Expr, JoinKind, QuerySource, SourceExecutionEnv};

#[test]
fn should_keep_join_source_output_charged_after_returning_to_the_consumer() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-join-source-ownership-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 1024 * 1024;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let source = QuerySource::Join {
        left: Box::new(QuerySource::SingleRow),
        right: Box::new(QuerySource::SingleRow),
        kind: JoinKind::Cross,
        on: Expr::BoolLiteral(true),
    };
    let mut cte_context = HashMap::new();

    // Act
    let (batches, text_fields) =
        execute_query_source(&env, &source, &mut cte_context, false, None, Some(1)).unwrap();

    // Assert
    assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 1);
    assert_eq!(text_fields, [] as [String; 0]);
    assert!(
        controls.current_query_memory_bytes() >= std::mem::size_of::<BatchRow>(),
        "retained returned join rows must own a reservation"
    );
    drop(batches);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_allocate_only_actual_row_slots_for_a_small_join_chunk() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-join-small-chunk-{}", uuid::Uuid::new_v4()));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 1024 * 1024;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let source = QuerySource::Join {
        left: Box::new(QuerySource::SingleRow),
        right: Box::new(QuerySource::SingleRow),
        kind: JoinKind::Cross,
        on: Expr::BoolLiteral(true),
    };
    let mut cte_context = HashMap::new();

    // Act
    let (batches, _) =
        execute_query_source(&env, &source, &mut cte_context, false, None, Some(1)).unwrap();

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
    assert_eq!(batches[0].capacity(), 1);
    drop(batches);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
