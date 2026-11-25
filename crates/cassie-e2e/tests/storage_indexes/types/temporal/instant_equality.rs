//! Timestamptz equality by instant through indexed and table scans.

use cassie::types::Value;

#[test]
fn should_match_indexed_results_given_equal_instants_with_different_offsets_when_timestamptz_rows_are_filtered(
) {
    // Arrange
    let cassie =
        crate::support_sql::new_in_memory_cassie().expect("create timestamptz index fixture");
    cassie.startup().expect("start timestamptz index fixture");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE issue_814_instant_scan (id INT4 PRIMARY KEY, occurred_at TIMESTAMPTZ)",
        "CREATE TABLE issue_814_instant_indexed (id INT4 PRIMARY KEY, occurred_at TIMESTAMPTZ)",
        "INSERT INTO issue_814_instant_scan VALUES \
            (1, TIMESTAMPTZ '2024-03-10 01:30:00-05'), \
            (2, TIMESTAMPTZ '2024-03-10 06:30:00+00'), \
            (3, TIMESTAMPTZ '2024-03-10 01:30:00-04')",
        "INSERT INTO issue_814_instant_indexed \
            SELECT id, occurred_at FROM issue_814_instant_scan",
        "CREATE INDEX issue_814_instant_indexed_at_idx \
            ON issue_814_instant_indexed USING btree (occurred_at)",
    ] {
        cassie
            .execute_sql(&session, sql, Vec::new())
            .unwrap_or_else(|error| panic!("setup statement failed: {sql}: {error}"));
    }

    // Act
    let scan = cassie
        .execute_sql(
            &session,
            "SELECT id FROM issue_814_instant_scan \
             WHERE occurred_at = TIMESTAMPTZ '2024-03-10 06:30:00+00' ORDER BY id",
            Vec::new(),
        )
        .expect("scan query");
    let indexed = cassie
        .execute_sql(
            &session,
            "SELECT id FROM issue_814_instant_indexed \
             WHERE occurred_at = TIMESTAMPTZ '2024-03-10 06:30:00+00' ORDER BY id",
            Vec::new(),
        )
        .expect("indexed query");

    // Assert
    let expected = vec![vec![Value::Int64(1)], vec![Value::Int64(2)]];
    assert_eq!(scan.rows(), expected);
    assert_eq!(indexed.rows(), expected);
}
