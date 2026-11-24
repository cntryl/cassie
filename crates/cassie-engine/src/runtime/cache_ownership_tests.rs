use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::{
    ExecutionMode, ExecutionResultCacheKey, QueryExecutionControls, RuntimeState,
};
use crate::types::Value;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn key() -> ExecutionResultCacheKey {
    ExecutionResultCacheKey {
        sql_fingerprint: 101,
        params_hash: 0,
        schema_epoch: 1,
        data_epoch: 1,
        user: "reader".to_owned(),
        database: Some("postgres".to_owned()),
        search_path: vec!["public".to_owned()],
        settings_fingerprint: 0,
        mode: ExecutionMode::SimpleQuery,
    }
}

fn result(controls: &QueryExecutionControls) -> QueryResult {
    let mut builder = QueryResult::try_builder(controls).expect("builder");
    builder
        .try_push_column(&ColumnMeta::text("payload"))
        .expect("column");
    builder
        .try_push_row(&[Value::String("owned".repeat(128))])
        .expect("row");
    builder.try_set_command("SELECT").expect("command");
    builder.finish().expect("result")
}

#[test]
fn should_reject_a_cache_hit_before_exposing_rows_to_an_underbudget_query() {
    // Arrange
    let producer = controls(1_048_576);
    let requester = controls(1);
    let runtime = RuntimeState::new(CassieRuntimeLimits::default());
    let key = key();
    runtime.execution_result_cache_store(&key, result(&producer));
    assert!(runtime.execution_result_cache_lookup(&key).is_some());
    let hits = runtime.snapshot().execution_result_cache.hits;

    // Act
    let denied = runtime.execution_result_cache_lookup_with_controls(&key, &requester);
    let after = runtime.snapshot();
    let subsequent = runtime
        .execution_result_cache_lookup(&key)
        .expect("entry survives denial");
    runtime.invalidate_execution_result_cache();
    drop(subsequent);

    // Assert
    assert!(matches!(denied, Err(CassieError::ResourceLimit(_))));
    assert_eq!(after.execution_result_cache.hits, hits);
    assert_eq!(requester.current_query_memory_bytes(), 0);
    assert_eq!(producer.current_query_memory_bytes(), 0);
}

#[test]
fn should_skip_optional_cache_insertion_before_unadmitted_capacity_growth() {
    // Arrange
    let producer = controls(1_048_576);
    let result = result(&producer);
    let occupied = producer
        .reserve_query_memory(1_048_576 - producer.current_query_memory_bytes())
        .expect("fill remaining budget");
    let runtime = RuntimeState::new(CassieRuntimeLimits::default());
    let key = key();

    // Act
    runtime.execution_result_cache_store(&key, result.clone());
    let snapshot = runtime.snapshot();
    let preserved = result.rows()[0][0] == Value::String("owned".repeat(128));
    runtime.invalidate_execution_result_cache();
    drop(result);
    drop(occupied);

    // Assert
    assert_eq!(snapshot.execution_result_cache.entries, 0);
    assert_eq!(
        snapshot
            .execution_result_cache
            .bypass_reasons
            .get("memory_budget"),
        Some(&1)
    );
    assert!(preserved);
    assert_eq!(producer.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_cache_backing_capacity_after_invalidation() {
    // Arrange
    let producer = controls(1_048_576);
    let runtime = RuntimeState::new(CassieRuntimeLimits::default());
    let result = result(&producer);
    let alias = result.clone();
    runtime.execution_result_cache_store(&key(), result);

    // Act
    runtime.invalidate_execution_result_cache();
    let cache = runtime.execution_result_cache.lock().expect("cache");
    let capacities = (cache.entries.capacity(), cache.order.capacity());
    drop(cache);
    let alias_still_admitted = producer.current_query_memory_bytes();
    drop(alias);

    // Assert
    assert_eq!(capacities, (0, 0));
    assert!(alias_still_admitted > 640);
    assert_eq!(producer.current_query_memory_bytes(), 0);
}

#[test]
fn should_count_a_shared_physical_payload_once_across_cached_keys() {
    // Arrange
    let producer = controls(1_048_576);
    let runtime = RuntimeState::new(CassieRuntimeLimits::default());
    let result = result(&producer);
    let mut second = key();
    second.sql_fingerprint += 1;

    // Act
    runtime.execution_result_cache_store(&key(), result.clone());
    runtime.execution_result_cache_store(&second, result.clone());
    let snapshot = runtime.snapshot();
    let admitted = producer.current_query_memory_bytes();
    runtime.invalidate_execution_result_cache();
    let invalidated = runtime.snapshot();
    drop(result);

    // Assert
    assert_eq!(snapshot.execution_result_cache.entries, 2);
    assert_eq!(
        snapshot.execution_result_cache.accounted_retained_bytes,
        u64::try_from(admitted).expect("counter")
    );
    assert!(
        snapshot.execution_result_cache.accounted_retained_bytes
            > snapshot.execution_result_cache.bytes
    );
    assert_eq!(
        invalidated.execution_result_cache.accounted_retained_bytes,
        0
    );
    assert_eq!(producer.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_resident_key_leases_when_a_different_producer_replaces_the_payload() {
    // Arrange
    let first = controls(1_048_576);
    let second = controls(1_048_576);
    let runtime = RuntimeState::new(CassieRuntimeLimits::default());
    runtime.execution_result_cache_store(&key(), result(&first));

    // Act
    runtime.execution_result_cache_store(&key(), result(&second));
    let first_live = first.current_query_memory_bytes();
    let second_live = second.current_query_memory_bytes();
    let snapshot = runtime.snapshot();
    runtime.invalidate_execution_result_cache();

    // Assert
    assert!(
        first_live > 0,
        "resident keys and directories retain their original producer lease"
    );
    assert!(
        second_live > 0,
        "replacement payload retains its own producer lease"
    );
    assert_eq!(
        snapshot.execution_result_cache.accounted_retained_bytes,
        u64::try_from(first_live + second_live).expect("counter")
    );
    assert_eq!(first.current_query_memory_bytes(), 0);
    assert_eq!(second.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_existing_cache_owners_unchanged_when_second_container_admission_fails() {
    // Arrange
    let producer = controls(1_048_576);
    let requester = controls(1_048_576);
    let runtime = RuntimeState::new(CassieRuntimeLimits::default());
    let result = result(&producer);
    for fingerprint in 0..4 {
        let mut key = key();
        key.sql_fingerprint = fingerprint;
        runtime.execution_result_cache_store(&key, result.clone());
    }
    let before = runtime.snapshot();
    let before_live = producer.current_query_memory_bytes();
    let candidate = result
        .try_clone_for_query(&requester)
        .expect("requesting view");
    let mut new_key = key();
    new_key.sql_fingerprint = 1000;
    let key_heap = new_key.user.capacity()
        + new_key.database.as_ref().expect("database").capacity()
        + new_key.search_path.capacity() * std::mem::size_of::<String>()
        + new_key
            .search_path
            .iter()
            .map(String::capacity)
            .sum::<usize>();
    let map_bytes = crate::executor::retained_memory::hash_table_bytes::<(
        ExecutionResultCacheKey,
        super::ExecutionResultCacheEntry,
    )>(8)
    .expect("map bound");
    let blocker = requester
        .reserve_query_memory(
            1_048_576 - requester.current_query_memory_bytes() - 2 * key_heap - map_bytes,
        )
        .expect("leave exactly keys plus first replacement container");
    let requester_before = requester.current_query_memory_bytes();

    // Act
    runtime.execution_result_cache_store(&new_key, candidate.clone());
    let after = runtime.snapshot();
    let requester_after = requester.current_query_memory_bytes();
    let producer_after = producer.current_query_memory_bytes();
    let cache = runtime.execution_result_cache.lock().expect("cache");
    let capacities = (cache.entries.capacity(), cache.order.capacity());
    let original_keys = (0..4).all(|fingerprint| {
        cache
            .entries
            .keys()
            .any(|key| key.sql_fingerprint == fingerprint)
    });
    drop(cache);
    runtime.invalidate_execution_result_cache();
    drop(candidate);
    drop(blocker);
    drop(result);

    // Assert
    assert_eq!(
        after.execution_result_cache.entries,
        before.execution_result_cache.entries
    );
    assert_eq!(
        after.execution_result_cache.bytes,
        before.execution_result_cache.bytes
    );
    assert_eq!(
        after.execution_result_cache.accounted_retained_bytes,
        before.execution_result_cache.accounted_retained_bytes
    );
    assert_eq!(
        after
            .execution_result_cache
            .bypass_reasons
            .get("memory_budget"),
        Some(&1)
    );
    assert_eq!(capacities, (7, 4));
    assert!(original_keys);
    assert_eq!(producer_after, before_live);
    assert_eq!(requester_after, requester_before);
    assert_eq!(requester.current_query_memory_bytes(), 0);
    assert_eq!(producer.current_query_memory_bytes(), 0);
}
