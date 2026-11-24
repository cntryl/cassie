//! Stream-level tests of the authoritative column vector source (#1358).

use super::super::{Cassie, CassieSession, QueryError, QueryExecutionControls, TypedScanStream};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::types::Value;
use std::time::Instant;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::from_env().expect("config"))
            .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE vec_src (n BIGINT, label TEXT) WITH (storage = column_store)",
        "INSERT INTO vec_src VALUES (1,'a'),(NULL,'b'),(3,NULL),(4,'d')",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("fixture");
    }
    (cassie, session)
}

fn controls(cassie: &Cassie) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now())
}

fn limited(bytes: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn read_all(
    cassie: &Cassie,
    session: &CassieSession,
    controls: &QueryExecutionControls,
) -> Result<Vec<Vec<Value>>, QueryError> {
    let mut stream = TypedScanStream::open(
        cassie,
        Some(session),
        "vec_src",
        &["n".to_owned(), "label".to_owned()],
        controls,
    )?
    .expect("typed stream");
    let mut rows = Vec::new();
    while let Some(batch) = stream.next_batch()? {
        for lane in 0..batch.len() {
            rows.push(vec![batch.value(0, lane)?, batch.value(1, lane)?]);
        }
    }
    rows.sort_by_key(|row| format!("{row:?}"));
    Ok(rows)
}

#[test]
fn should_read_the_same_values_from_vector_batches_as_from_documents() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls(&cassie);
    let before = cassie.midge.column_vector_reads_for_diagnostics();

    // Act
    let vector = read_all(&cassie, &session, &controls).expect("vector read");
    let delta = cassie
        .midge
        .column_vector_reads_for_diagnostics()
        .since(before);
    cassie
        .midge
        .set_column_vector_enabled_for_diagnostics(false);
    let documents = read_all(&cassie, &session, &controls).expect("document read");
    cassie.midge.set_column_vector_enabled_for_diagnostics(true);

    // Assert
    assert_eq!(vector, documents);
    assert_eq!(vector.len(), 4);
    assert_eq!(delta.batches, 1);
    assert_eq!(delta.row_documents_built, 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_merge_staged_changes_in_id_order() {
    // Arrange
    let (cassie, session) = fixture();
    for sql in [
        "BEGIN",
        "INSERT INTO vec_src VALUES (10,'new')",
        "UPDATE vec_src SET label = 'changed' WHERE n = 1",
        "DELETE FROM vec_src WHERE n = 4",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("transaction");
    }
    let controls = controls(&cassie);
    let before = cassie.midge.column_vector_reads_for_diagnostics();

    // Act
    let vector = read_all(&cassie, &session, &controls).expect("vector read");
    let delta = cassie
        .midge
        .column_vector_reads_for_diagnostics()
        .since(before);
    cassie
        .midge
        .set_column_vector_enabled_for_diagnostics(false);
    let documents = read_all(&cassie, &session, &controls).expect("document read");
    cassie.midge.set_column_vector_enabled_for_diagnostics(true);

    // Assert
    assert_eq!(vector, documents);
    assert!(vector.contains(&vec![Value::Int64(10), Value::String("new".to_owned())]));
    assert!(!vector.iter().any(|row| row[0] == Value::Int64(4)));
    assert_eq!(delta.staged_rows_merged, 3);
    assert_eq!(delta.row_documents_built, 0);
}

#[test]
fn should_return_no_batch_when_the_query_is_canceled_mid_scan() {
    // Arrange
    let (cassie, session) = fixture();
    let handle = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &cassie.runtime.limits(),
        Instant::now(),
        handle.clone(),
    );
    let mut stream = TypedScanStream::open(
        &cassie,
        Some(&session),
        "vec_src",
        &["n".to_owned()],
        &controls,
    )
    .expect("open")
    .expect("typed stream");
    handle.cancel();

    // Act
    let result = stream.next_batch();

    // Assert
    assert!(result.is_err(), "a canceled scan returns no batch");
    drop(stream);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_fit_a_budget_equal_to_the_observed_peak() {
    // Arrange
    let (cassie, session) = fixture();
    let probe = controls(&cassie);
    read_all(&cassie, &session, &probe).expect("probe");
    let peak = probe.peak_query_memory_bytes();
    let controls = limited(peak);

    // Act
    let result = read_all(&cassie, &session, &controls);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn should_reject_a_budget_one_byte_below_the_observed_peak_with_no_batch() {
    // Arrange
    let (cassie, session) = fixture();
    let probe = controls(&cassie);
    read_all(&cassie, &session, &probe).expect("probe");
    let peak = probe.peak_query_memory_bytes();
    let controls = limited(peak - 1);
    let mut batches = 0usize;

    // Act
    let result = (|| -> Result<(), QueryError> {
        let mut stream = TypedScanStream::open(
            &cassie,
            Some(&session),
            "vec_src",
            &["n".to_owned(), "label".to_owned()],
            &controls,
        )?
        .expect("typed stream");
        while stream.next_batch()?.is_some() {
            batches += 1;
        }
        Ok(())
    })();

    // Assert
    assert!(result.is_err());
    assert_eq!(batches, 0, "no partial output");
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
