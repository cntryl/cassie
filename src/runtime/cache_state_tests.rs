use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::ColumnMeta;
use crate::executor::QueryResult;
use crate::runtime::ExecutionMode;
use crate::types::Value;

fn key(id: u64) -> ExecutionResultCacheKey {
    ExecutionResultCacheKey {
        sql_fingerprint: id,
        params_hash: 0,
        schema_epoch: 1,
        data_epoch: 1,
        user: "reader".into(),
        database: Some("postgres".into()),
        search_path: vec!["public".into()],
        settings_fingerprint: 0,
        mode: ExecutionMode::SimpleQuery,
    }
}

fn result(width: usize) -> QueryResult {
    let controls = super::super::QueryExecutionControls::from_limits(
        &CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let mut builder = QueryResult::try_builder(&controls).expect("builder");
    builder
        .try_push_column(&ColumnMeta::text("payload"))
        .expect("column");
    builder
        .try_push_row(&[Value::String("x".repeat(width))])
        .expect("row");
    builder.try_set_command("SELECT 1").expect("command");
    builder.finish().expect("result")
}

fn payload_size(result: &QueryResult) -> usize {
    serde_json::to_vec(result)
        .expect("serializable result")
        .len()
}

fn assert_payload(runtime: &RuntimeState, key: &ExecutionResultCacheKey, width: usize) {
    let cached = runtime
        .execution_result_cache_lookup(key)
        .expect("cache hit");
    assert_eq!(cached.rows(), result(width).rows());
    assert_eq!(cached.columns(), vec![ColumnMeta::text("payload")]);
    assert_eq!(cached.command(), "SELECT 1");
}

#[test]
fn should_account_variable_width_replacement_under_the_result_cache_byte_limit() {
    // Arrange
    let small = result(64);
    let medium = result(4_096);
    let large = result(8_192);
    let limit = payload_size(&small) + payload_size(&large);
    let runtime = RuntimeState::new(CassieRuntimeLimits {
        execution_result_cache_max_entries: 8,
        execution_result_cache_max_bytes: limit,
        ..CassieRuntimeLimits::default()
    });
    let first = key(1);
    let second = key(2);
    runtime.execution_result_cache_store(&first, small.clone());
    runtime.execution_result_cache_store(&second, medium.clone());
    assert_eq!(
        runtime.execution_result_cache_size(),
        (2, payload_size(&small) + payload_size(&medium))
    );

    // Act
    runtime.execution_result_cache_store(&first, large.clone());

    // Assert
    assert_eq!(
        runtime.execution_result_cache_size(),
        (1, payload_size(&large))
    );
    assert!(runtime.execution_result_cache_lookup(&second).is_none());
    assert_payload(&runtime, &first, 8_192);
    runtime.execution_result_cache_store(&first, small.clone());
    assert_eq!(
        runtime.execution_result_cache_size(),
        (1, payload_size(&small))
    );
    runtime.execution_result_cache_store(&second, large);
    assert_eq!(runtime.execution_result_cache_size(), (2, limit));
    assert_payload(&runtime, &first, 64);
    assert_payload(&runtime, &second, 8_192);
    runtime.invalidate_execution_result_cache();
    assert_eq!(runtime.execution_result_cache_size(), (0, 0));
    let cache = runtime.execution_result_cache.lock().expect("cache state");
    assert!(cache.entries.is_empty());
    assert!(cache.order.is_empty());
}

#[test]
fn should_preserve_returned_result_ownership_after_cache_retirement() {
    // Arrange
    let runtime = RuntimeState::new(CassieRuntimeLimits {
        execution_result_cache_max_entries: 2,
        execution_result_cache_max_bytes: 1_000_000,
        ..CassieRuntimeLimits::default()
    });
    let first = key(1);
    let second = key(2);
    let third = key(3);
    runtime.execution_result_cache_store(&first, result(64));
    runtime.execution_result_cache_store(&second, result(4_096));
    let mut returned = runtime
        .execution_result_cache_lookup(&first)
        .expect("actual hit");
    let controls = super::super::QueryExecutionControls::from_limits(
        &CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    returned = returned
        .try_with_rows(&controls, &[vec![Value::String("caller mutation".into())]])
        .expect("admitted replacement");

    // Act
    runtime.execution_result_cache_store(&third, result(8_192));

    // Assert
    assert!(
        runtime.execution_result_cache_lookup(&second).is_none(),
        "actual LRU entry retired"
    );
    assert_payload(&runtime, &first, 64);
    assert_payload(&runtime, &third, 8_192);
    assert_eq!(
        runtime.execution_result_cache_size(),
        (2, payload_size(&result(64)) + payload_size(&result(8_192)))
    );
    runtime.invalidate_execution_result_cache();
    assert_eq!(runtime.execution_result_cache_size(), (0, 0));
    assert!(runtime.execution_result_cache_lookup(&first).is_none());
    assert!(runtime.execution_result_cache_lookup(&third).is_none());
    assert_eq!(
        returned.rows(),
        vec![vec![Value::String("caller mutation".into())]],
        "caller owns its independent result after cache retirement"
    );
    drop(returned);
    let cache = runtime.execution_result_cache.lock().expect("cache state");
    assert!(cache.entries.is_empty());
    assert!(cache.order.is_empty());
}
