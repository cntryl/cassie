use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;
use cassie::CassieRuntimeConfig;

const QUERY: &str = "SELECT marker FROM cache_tx_qualified ORDER BY marker";

fn metric(cassie: &Cassie, field: &str) -> u64 {
    cassie.metrics()["execution_result_cache"][field]
        .as_u64()
        .expect("present cache counter")
}

fn rows(cassie: &Cassie, session: &CassieSession, expected: &[&str]) {
    assert_eq!(
        cassie
            .execute_sql(session, QUERY, vec![])
            .expect("query")
            .rows(),
        expected
            .iter()
            .map(|value| vec![Value::String((*value).into())])
            .collect::<Vec<_>>()
    );
}

fn assert_transaction_read(cassie: &Cassie, session: &CassieSession, expected: &[&str]) {
    let hits = metric(cassie, "hits");
    let misses = metric(cassie, "misses");
    let before = cassie.metrics()["execution_result_cache"]["bypass_reasons"]["active_transaction"]
        .as_u64()
        .unwrap_or(0);
    rows(cassie, session, expected);
    assert_eq!(
        metric(cassie, "hits"),
        hits,
        "transaction never consumes warm cache"
    );
    assert_eq!(
        metric(cassie, "misses"),
        misses,
        "transaction bypasses cache lookup"
    );
    assert_eq!(
        cassie.metrics()["execution_result_cache"]["bypass_reasons"]["active_transaction"].as_u64(),
        Some(before + 1),
        "each transaction read records an actual bypass"
    );
}

#[test]
fn should_bypass_a_proven_warm_result_cache_through_savepoint_rollback() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    let session = cassie.create_session("cache_reader", None);
    for sql in [
        "CREATE TABLE cache_tx_qualified (marker TEXT)",
        "INSERT INTO cache_tx_qualified VALUES ('before')",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    rows(&cassie, &session, &["before"]);
    let before_warm = metric(&cassie, "hits");
    rows(&cassie, &session, &["before"]);
    assert_eq!(
        metric(&cassie, "hits"),
        before_warm + 1,
        "prove warm cache hit"
    );

    // Act
    for sql in ["BEGIN", "INSERT INTO cache_tx_qualified VALUES ('during')"] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("transaction stage");
    }
    assert_transaction_read(&cassie, &session, &["before", "during"]);
    for sql in [
        "SAVEPOINT cache_point",
        "INSERT INTO cache_tx_qualified VALUES ('extra')",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("savepoint stage");
    }
    assert_transaction_read(&cassie, &session, &["before", "during", "extra"]);
    cassie
        .execute_sql(&session, "ROLLBACK TO SAVEPOINT cache_point", vec![])
        .expect("savepoint rollback");
    assert_transaction_read(&cassie, &session, &["before", "during"]);
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("transaction rollback");

    // Assert
    rows(&cassie, &session, &["before"]);
    let hits = metric(&cassie, "hits");
    rows(&cassie, &session, &["before"]);
    assert_eq!(
        metric(&cassie, "hits"),
        hits + 1,
        "cache usable after rollback"
    );
    drop(session);
    cassie.shutdown();
    drop(cassie);
}
