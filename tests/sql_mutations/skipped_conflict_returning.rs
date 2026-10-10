//! Finite CON-10 qualification for mixed skipped/inserted VALUES and fresh retry.

use super::support_sql_fixture::sql_fixture;
use cassie::executor::ColumnMeta;
use cassie::types::{DataType, Value};

const INSERT: &str = "INSERT INTO skipped_returning (id,note) \
    VALUES (3,'third'),(2,'ignored'),(1,'first') \
    ON CONFLICT (id) DO NOTHING RETURNING id AS row_key,note";
const OBSERVE: &str = "SELECT id,note FROM skipped_returning ORDER BY id";

#[test]
fn should_preserve_skipped_conflict_insert_returning_contract() {
    // Arrange
    let fixture = sql_fixture("skipped-conflict-returning", &[]);
    fixture.cassie.startup().expect("startup");
    for sql in [
        "CREATE TABLE skipped_returning (id BIGINT PRIMARY KEY,note TEXT)",
        "INSERT INTO skipped_returning (id,note) VALUES (2,'old')",
    ] {
        fixture.execute(sql).expect("seed conflict owner");
    }
    let expected_columns = vec![
        ColumnMeta::from_data_type("row_key", &DataType::BigInt),
        ColumnMeta::from_data_type("note", &DataType::Text),
    ];
    let seed_observer = fixture.cassie.create_session("seed-observer", None);
    assert_eq!(
        fixture
            .cassie
            .execute_sql(&seed_observer, OBSERVE, vec![])
            .expect("independent seed state")
            .rows,
        vec![vec![Value::Int64(2), Value::String("old".into())]],
    );
    let expected_state = vec![
        vec![Value::Int64(1), Value::String("first".into())],
        vec![Value::Int64(2), Value::String("old".into())],
        vec![Value::Int64(3), Value::String("third".into())],
    ];

    // Act
    let before_first = fixture.cassie.metrics();
    let first = fixture
        .execute(INSERT)
        .expect("mixed skipped/inserted command");
    let after_first = fixture.cassie.metrics();
    let first_observer = fixture.cassie.create_session("first-observer", None);
    let first_state = fixture
        .cassie
        .execute_sql(&first_observer, OBSERVE, vec![])
        .expect("independent committed first state");
    let before_retry = fixture.cassie.metrics();
    let retry = fixture.execute(INSERT).expect("fresh all-skipped command");
    let after_retry = fixture.cassie.metrics();
    let retry_observer = fixture.cassie.create_session("retry-observer", None);
    let retry_state = fixture
        .cassie
        .execute_sql(&retry_observer, OBSERVE, vec![])
        .expect("independent committed retry state");

    // Assert
    for (before, after, rows) in [
        (&before_first, &after_first, 2),
        (&before_retry, &after_retry, 0),
    ] {
        assert_eq!(
            after["query"]["count"].as_u64(),
            Some(before["query"]["count"].as_u64().expect("success count") + 1)
        );
        assert_eq!(
            after["query"]["rows_returned_total"].as_u64(),
            Some(
                before["query"]["rows_returned_total"]
                    .as_u64()
                    .expect("returned rows")
                    + rows
            )
        );
        assert_eq!(
            after["query"]["errors_total"].as_u64(),
            Some(
                before["query"]["errors_total"]
                    .as_u64()
                    .expect("error count")
            )
        );
    }
    assert_eq!(first.command, "INSERT 0 2");
    assert_eq!(
        first.rows,
        vec![
            vec![Value::Int64(3), Value::String("third".into())],
            vec![Value::Int64(1), Value::String("first".into())],
        ]
    );
    assert!(first.rows.iter().all(|row| row.len() == 2));
    assert_eq!(first.columns, expected_columns);
    assert_eq!(retry.command, "INSERT 0 0");
    assert_eq!(retry.rows, Vec::<Vec<Value>>::new());
    assert_eq!(retry.columns, first.columns);
    assert_eq!(first_state.rows, expected_state);
    assert_eq!(retry_state.rows, expected_state);
    assert!(first_state.rows.iter().all(|row| row.len() == 2));
    assert!(retry_state.rows.iter().all(|row| row.len() == 2));
}
