use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::{DataType, Value};

use super::{AccountedVectorResult, Cassie, CassieError, VectorSearchDiagnostics};

#[test]
fn should_reject_vector_result_cancelled_during_final_conversion() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-vector-finalizer-cancellation-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie = Cassie::new_with_data_dir_and_config(&path, CassieRuntimeConfig::default())
        .expect("ephemeral Cassie");
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation.clone(),
    );
    let accounted = accounted_result(&controls);
    let retained_bytes = controls.current_query_memory_bytes();
    let before = cassie.runtime.snapshot();
    let conversion_calls = Cell::new(0);

    // Act
    let result = cassie.finalize_vector_search(accounted, &controls, |result, _| {
        assert_eq!(controls.current_query_memory_bytes(), retained_bytes);
        assert!(!controls.is_cancelled());
        conversion_calls.set(conversion_calls.get() + 1);
        cancellation.cancel();
        assert_eq!(result.rows(), vec![vec![Value::Int64(7)]]);
        Ok(result)
    });
    let after = cassie.runtime.snapshot();
    let live_bytes = controls.current_query_memory_bytes();
    drop(cassie);
    if path.exists() {
        std::fs::remove_dir_all(path).expect("remove ephemeral Cassie");
    }

    // Assert
    assert_eq!(conversion_calls.get(), 1);
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(after.vector.count, before.vector.count);
    assert_eq!(after.vector.hnsw_executions, before.vector.hnsw_executions);
    assert_eq!(
        after.vector.normalized_candidate_count_total,
        before.vector.normalized_candidate_count_total
    );
    assert_eq!(live_bytes, 0);
}

fn accounted_result(controls: &QueryExecutionControls) -> AccountedVectorResult {
    AccountedVectorResult {
        memory: controls
            .reserve_query_memory(1024)
            .expect("metadata admission"),
        row_memory: Some(controls.reserve_query_memory(1024).expect("row admission")),
        result: {
            let mut result = QueryResult::try_builder(controls).expect("builder");
            result
                .try_push_column(&ColumnMeta::from_data_type("value", &DataType::BigInt))
                .expect("column");
            result.try_push_row(&[Value::Int64(7)]).expect("row");
            result.try_set_command("SELECT").expect("command");
            result.finish().expect("result")
        },
        diagnostics: VectorSearchDiagnostics {
            normalization: Some((1, 0)),
            execution: Some((Duration::ZERO, 1, 1)),
            hnsw: Some(1),
            ivfflat: None,
        },
    }
}
#[test]
fn should_enforce_cached_vector_result_controls_before_retained_copy() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-vector-cached-controls-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie = Cassie::new_with_data_dir_and_config(&path, CassieRuntimeConfig::default())
        .expect("ephemeral Cassie");
    let producer = cached_result_controls(usize::MAX, 1_048_576);
    let key = cached_result_key(&cassie, &producer);
    let lookup_key = key.key.clone();
    let expected_rows = vec![vec![Value::Int64(7)], vec![Value::Int64(8)]];
    let mut result = QueryResult::try_builder(&producer).expect("builder");
    result
        .try_push_column(&ColumnMeta::from_data_type("value", &DataType::BigInt))
        .expect("column");
    for row in &expected_rows {
        result.try_push_row(row).expect("row");
    }
    result.try_set_command("SELECT").expect("command");
    let cached = result.finish().expect("result");
    cassie.cache_vector_search_result_controlled(key, &cached, &producer);
    let cache_present = cassie
        .vector_search_result_cache
        .lock()
        .contains_key(&lookup_key);
    let original_producer_bytes = producer.current_query_memory_bytes();
    let controls = [
        cached_result_controls(1, 1),
        cached_result_controls(2, 1),
        cached_result_controls(2, 1024 * 1024),
    ];
    let before = cassie.runtime.snapshot();

    // Act
    let [row_cap, memory_cap, successful] = controls
        .each_ref()
        .map(|controls| cassie.cached_vector_search_result_controlled(&lookup_key, controls));
    let successful = successful
        .expect("sufficient controls")
        .expect("actual cache hit");
    let admitted_bytes = controls[2].current_query_memory_bytes();
    let copied_rows = successful.result.rows().to_vec();
    drop(successful);
    let released = controls
        .each_ref()
        .map(QueryExecutionControls::current_query_memory_bytes);
    let failed_peaks = [
        controls[0].peak_query_memory_bytes(),
        controls[1].peak_query_memory_bytes(),
    ];
    let final_producer_bytes = producer.current_query_memory_bytes();
    let cache_preserved = cassie
        .vector_search_result_cache
        .lock()
        .get(&lookup_key)
        .is_some_and(|resident| resident.shares_storage(&cached));
    let after = cassie.runtime.snapshot();
    drop(cassie);
    if path.exists() {
        std::fs::remove_dir_all(path).expect("remove ephemeral Cassie");
    }

    // Assert
    assert!(
        cache_present && cache_preserved,
        "probe must use a retained cache entry"
    );
    assert!(
        matches!(&row_cap, Err(CassieError::ResourceLimit(message))
        if message == "query result row limit exceeded: 2 > 1"),
        "{:?}",
        row_cap.as_ref().err()
    );
    assert!(
        matches!(&memory_cap, Err(CassieError::ResourceLimit(message))
        if message.starts_with("query memory budget exceeded:")),
        "{:?}",
        memory_cap.as_ref().err()
    );
    assert_eq!(
        failed_peaks,
        [0, 0],
        "rejection precedes retained-copy admission"
    );
    assert!(
        admitted_bytes > 0,
        "returned copy must keep its reservation alive"
    );
    assert_eq!(
        copied_rows, expected_rows,
        "requested limit 10 is legal for two actual rows"
    );
    assert_eq!(released, [0, 0, 0]);
    assert_eq!(final_producer_bytes, original_producer_bytes);
    assert_eq!(after.vector.count, before.vector.count);
}

fn cached_result_key(
    cassie: &Cassie,
    producer: &QueryExecutionControls,
) -> super::AdmittedVectorResultKey {
    super::AdmittedVectorResultKey::new(
        cassie.catalog.version(),
        "local",
        "cached-controls",
        3,
        super::VectorSearchInput {
            collection: "docs",
            vector_field: "embedding",
            query: "query",
            metric: Some(super::DistanceMetric::L2),
            limit: 10,
            offset: 0,
        },
        super::DistanceMetric::L2,
        producer,
    )
    .expect("key admission")
}

fn cached_result_controls(max_result_rows: usize, memory_budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            max_result_rows,
            query_memory_budget_bytes: memory_budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}
