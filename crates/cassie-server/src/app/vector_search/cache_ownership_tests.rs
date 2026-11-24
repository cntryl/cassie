use std::path::PathBuf;
use std::time::Instant;

use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

use super::{AdmittedVectorResultKey, Cassie, DistanceMetric, VectorSearchInput};

const BUDGET: usize = 1024 * 1024;

struct Fixture {
    cassie: Option<Cassie>,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-vector-cache-owner-{}",
            uuid::Uuid::new_v4()
        ));
        let cassie = Cassie::new_with_data_dir_and_config(&path, CassieRuntimeConfig::default())
            .expect("ephemeral Cassie");
        Self {
            cassie: Some(cassie),
            path,
        }
    }

    fn cassie(&self) -> &Cassie {
        self.cassie.as_ref().expect("live fixture")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.cassie.take());
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path).expect("retired fixture cleanup");
        }
    }
}

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: BUDGET,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn result(controls: &QueryExecutionControls) -> QueryResult {
    let mut builder = QueryResult::try_builder(controls).expect("payload admission");
    builder
        .try_push_column(&ColumnMeta::text("value"))
        .expect("column");
    builder.try_push_row(&[Value::Int64(7)]).expect("row");
    builder.try_set_command("SELECT").expect("command");
    builder.finish().expect("managed result")
}

fn key(cassie: &Cassie, controls: &QueryExecutionControls, query: &str) -> AdmittedVectorResultKey {
    AdmittedVectorResultKey::new(
        cassie.catalog.version(),
        "local",
        "ownership",
        3,
        VectorSearchInput {
            collection: "docs",
            vector_field: "embedding",
            query,
            metric: Some(DistanceMetric::L2),
            limit: 10,
            offset: 0,
        },
        DistanceMetric::L2,
        controls,
    )
    .expect("transient key admission")
}

#[test]
fn should_retain_vector_result_cache_keys_until_actual_cache_retirement() {
    // Arrange
    let fixture = Fixture::new();
    let cassie = fixture.cassie();
    let controls = controls();
    let result = result(&controls);
    let payload = result.retained_bytes();
    let key = key(cassie, &controls, &"long query".repeat(1024));

    // Act
    cassie.cache_vector_search_result_controlled(key, &result, &controls);
    drop(result);
    let entries = cassie.vector_search_result_cache.lock().len();
    let retained = controls.current_query_memory_bytes();
    let retired = std::mem::take(&mut *cassie.vector_search_result_cache.lock());
    drop(retired);
    let released = controls.current_query_memory_bytes();
    drop(fixture);

    // Assert
    assert_eq!(
        entries, 1,
        "the probe must retain an actual production cache entry"
    );
    assert!(
        retained > payload,
        "resident key, directory and owner storage must remain admitted beyond the shared payload"
    );
    assert_eq!(released, 0);
}

#[test]
fn should_record_optional_vector_result_cache_budget_bypass_without_mutation() {
    // Arrange
    let fixture = Fixture::new();
    let cassie = fixture.cassie();
    let controls = controls();
    let result = result(&controls);
    cassie.cache_vector_search_result_controlled(
        key(cassie, &controls, "first"),
        &result,
        &controls,
    );
    let capacity = cassie.vector_search_result_cache.lock().capacity();
    for index in 1..capacity {
        let query = format!("resident-{index}");
        cassie.cache_vector_search_result_controlled(
            key(cassie, &controls, &query),
            &result,
            &controls,
        );
    }
    let original_keys = cassie
        .vector_search_result_cache
        .lock()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let second = key(cassie, &controls, "next-growth");
    let before = cassie
        .runtime
        .snapshot()
        .execution_result_cache
        .bypass_reasons
        .get("memory_budget")
        .copied()
        .unwrap_or_default();
    let blocker = controls
        .reserve_query_memory(BUDGET - controls.current_query_memory_bytes())
        .expect("occupy remaining budget");

    // Act
    cassie.cache_vector_search_result_controlled(second, &result, &controls);
    let resident_keys = cassie
        .vector_search_result_cache
        .lock()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let source_preserved = result.rows()[0][0] == Value::Int64(7);
    let after = cassie
        .runtime
        .snapshot()
        .execution_result_cache
        .bypass_reasons
        .get("memory_budget")
        .copied()
        .unwrap_or_default();
    drop(blocker);
    drop(result);
    let retired = std::mem::take(&mut *cassie.vector_search_result_cache.lock());
    drop(retired);
    let released = controls.current_query_memory_bytes();
    drop(fixture);

    // Assert
    assert!(source_preserved);
    assert!(
        capacity > 0,
        "the probe must fill an actual directory before denying its growth"
    );
    assert_eq!(original_keys.len(), capacity);
    assert_eq!(
        resident_keys, original_keys,
        "denied optional insertion must preserve resident identities"
    );
    assert_eq!(after, before + 1);
    assert_eq!(released, 0);
}

#[test]
fn should_preserve_vector_cache_producer_ownership_during_payload_replacement() {
    // Arrange
    let fixture = Fixture::new();
    let cassie = fixture.cassie();
    let producer = controls();
    let requester = controls();
    let original = result(&producer);
    let original_payload = original.retained_bytes();
    cassie.cache_vector_search_result_controlled(
        key(cassie, &producer, "same"),
        &original,
        &producer,
    );
    let original_cache_bytes = producer.current_query_memory_bytes() - original_payload;
    let replacement = result(&requester);
    let replacement_payload = replacement.retained_bytes();
    let incoming = key(cassie, &requester, "same");
    let lookup_key = incoming.key.clone();

    // Act
    cassie.cache_vector_search_result_controlled(incoming, &replacement, &requester);
    let producer_after_replace = producer.current_query_memory_bytes();
    let requester_after_replace = requester.current_query_memory_bytes();
    let cache_matches = cassie
        .vector_search_result_cache
        .lock()
        .get(&lookup_key)
        .is_some_and(|cached| cached.shares_storage(&replacement));
    drop(original);
    let producer_after_original = producer.current_query_memory_bytes();
    let retired = std::mem::take(&mut *cassie.vector_search_result_cache.lock());
    drop(retired);
    let producer_after_cache = producer.current_query_memory_bytes();
    let requester_after_cache = requester.current_query_memory_bytes();
    drop(replacement);
    let requester_released = requester.current_query_memory_bytes();
    drop(fixture);

    // Assert
    assert!(cache_matches);
    assert!(original_cache_bytes > 0);
    assert_eq!(
        producer_after_replace,
        original_cache_bytes + original_payload
    );
    assert_eq!(
        requester_after_replace, replacement_payload,
        "incoming duplicate key must retire"
    );
    assert_eq!(
        producer_after_original, original_cache_bytes,
        "resident key and directory keep their producer"
    );
    assert_eq!(producer_after_cache, 0);
    assert_eq!(
        requester_after_cache, replacement_payload,
        "external replacement alias still owns its payload"
    );
    assert_eq!(requester_released, 0);
}

#[test]
fn should_transfer_vector_cache_directory_ownership_during_cross_producer_growth() {
    // Arrange
    let fixture = Fixture::new();
    let cassie = fixture.cassie();
    let producer = controls();
    let requester = controls();
    let original = result(&producer);
    let original_payload = original.retained_bytes();
    cassie.cache_vector_search_result_controlled(
        key(cassie, &producer, "old-0"),
        &original,
        &producer,
    );
    let capacity = cassie.vector_search_result_cache.lock().capacity();
    for index in 1..capacity {
        let query = format!("old-{index}");
        cassie.cache_vector_search_result_controlled(
            key(cassie, &producer, &query),
            &original,
            &producer,
        );
    }
    let original_keys = cassie
        .vector_search_result_cache
        .lock()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let old_directory = cassie.vector_search_result_cache.lock().directory_bytes();
    let producer_before = producer.current_query_memory_bytes();
    let replacement = result(&requester);
    let replacement_payload = replacement.retained_bytes();
    let incoming = key(cassie, &requester, "new-growth");
    let lookup_key = incoming.key.clone();
    let inline_key = std::mem::size_of::<super::VectorSearchResultCacheKey>();
    let incoming_heap = requester.current_query_memory_bytes() - replacement_payload - inline_key;

    // Act
    cassie.cache_vector_search_result_controlled(incoming, &replacement, &requester);
    let cache = cassie.vector_search_result_cache.lock();
    let new_capacity = cache.capacity();
    let new_directory = cache.directory_bytes();
    let retained_entries = cache.len();
    let old_payloads_preserved = original_keys.iter().all(|key| {
        cache
            .get(key)
            .is_some_and(|result| result.shares_storage(&original))
    });
    let new_payload_preserved = cache
        .get(&lookup_key)
        .is_some_and(|result| result.shares_storage(&replacement));
    drop(cache);
    let producer_after_growth = producer.current_query_memory_bytes();
    let requester_after_growth = requester.current_query_memory_bytes();
    let retired = std::mem::take(&mut *cassie.vector_search_result_cache.lock());
    drop(retired);
    let producer_after_cache = producer.current_query_memory_bytes();
    let requester_after_cache = requester.current_query_memory_bytes();
    drop(original);
    drop(replacement);
    let producer_released = producer.current_query_memory_bytes();
    let requester_released = requester.current_query_memory_bytes();
    drop(fixture);

    // Assert
    assert!(
        capacity > 0,
        "the probe must fill an actual initial directory"
    );
    assert_eq!(original_keys.len(), capacity);
    assert!(old_directory > 0);
    assert_eq!(new_capacity, capacity * 2);
    assert_eq!(new_directory, old_directory * 2);
    assert_eq!(retained_entries, capacity + 1);
    assert!(old_payloads_preserved);
    assert!(new_payload_preserved);
    assert_eq!(
        producer_after_growth,
        producer_before - old_directory,
        "old directory backing retires while resident P keys and payload stay owned"
    );
    assert_eq!(
        requester_after_growth,
        replacement_payload + incoming_heap + new_directory,
        "Q owns only its new key, physical payload and complete replacement directory"
    );
    assert_eq!(
        producer_after_cache, original_payload,
        "dropping the cache releases P keys, retaining only the actual external payload alias"
    );
    assert_eq!(
        requester_after_cache, replacement_payload,
        "dropping the cache releases Q key and directory, retaining its external payload alias"
    );
    assert_eq!(producer_released, 0);
    assert_eq!(requester_released, 0);
}
