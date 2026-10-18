use super::support_read_equivalence::{sql, with_fixture};

#[test]
fn should_preserve_column_store_reads_with_time_series_indexes() {
    // Arrange
    with_fixture("column_time_series_equivalence", |cassie, session| {
        sql(cassie, session, "CREATE TABLE column_time_events (tenant TEXT,event_at TIMESTAMP,amount INT) WITH (storage = column_store)");
        sql(cassie, session, "INSERT INTO column_time_events VALUES ('a','2026-01-01T12:00:00Z',10),('a','2026-01-01T13:00:00Z',20),('b','2026-01-01T12:00:00Z',30)");
        let query = "SELECT tenant,event_at,amount FROM column_time_events WHERE tenant = 'a' AND event_at >= '2026-01-01T12:00:00Z' ORDER BY event_at LIMIT 2";
        let baseline = sql(cassie, session, query);
        sql(cassie, session, "CREATE INDEX column_time_events_idx ON column_time_events USING time_series(event_at) WITH (bucket_width = '1 hour',partition_by = tenant)");
        // Act
        let indexed = sql(cassie, session, query);
        // Assert
        assert_eq!(indexed.rows(), baseline.rows());
    });
}
