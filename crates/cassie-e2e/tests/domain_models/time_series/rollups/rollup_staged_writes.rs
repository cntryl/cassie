//! A rollup never answers a query in a session that has uncommitted changes to its source.

use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

const TOTALS: &str =
    "SELECT time_bucket('1 day', ts) AS bucket, tenant, COUNT(*) AS total FROM rsrc \
     GROUP BY time_bucket('1 day', ts), tenant ORDER BY bucket, tenant";

fn fresh_rollup(label: &str) -> SqlFixture {
    let fixture = sql_fixture(
        label,
        &[
            "CREATE TABLE rsrc (tenant TEXT, ts TIMESTAMP, n INT)",
            "INSERT INTO rsrc VALUES ('a', '2024-01-01 00:05:00', 1), ('a', '2024-01-01 00:10:00', 2)",
            "CREATE ROLLUP rr ON rsrc USING time_bucket('1 day', ts) GROUP BY tenant AGGREGATES COUNT(*) AS total",
            "REFRESH ROLLUP rr",
        ],
    );
    fixture.execute("BEGIN").expect("begin");
    fixture
}

fn copy_row(fixture: &SqlFixture) {
    let statement = CopyStatement {
        table: "rsrc".to_string(),
        columns: vec!["tenant".to_string(), "ts".to_string(), "n".to_string()],
        format: CopyFormat::Csv,
        header: false,
    };
    fixture
        .cassie
        .copy_from_csv_stdin(&fixture.session, &statement, b"a,2024-01-01 00:20:00,3\n")
        .expect("stage COPY");
}

fn first_total(fixture: &SqlFixture) -> Value {
    fixture.rows(TOTALS)[0][2].clone()
}

#[test]
fn should_count_rows_staged_by_copy_in_a_transaction() {
    // Arrange
    let fixture = fresh_rollup("rollup-copy-staged");

    // Act
    copy_row(&fixture);
    let total = first_total(&fixture);

    // Assert
    assert_eq!(total, Value::Int64(3));
}

#[test]
fn should_count_rows_staged_by_insert_in_a_transaction() {
    // Arrange
    let fixture = fresh_rollup("rollup-insert-staged");

    // Act
    fixture
        .execute("INSERT INTO rsrc VALUES ('a', '2024-01-01 00:20:00', 3)")
        .expect("insert");
    let total = first_total(&fixture);

    // Assert
    assert_eq!(total, Value::Int64(3));
}

#[test]
fn should_hide_rows_removed_by_truncate_in_a_transaction() {
    // Arrange
    let fixture = fresh_rollup("rollup-truncate-staged");

    // Act
    fixture.execute("TRUNCATE rsrc").expect("truncate");
    let rows = fixture.rows(TOTALS);

    // Assert
    assert_eq!(rows, Vec::<Vec<Value>>::new());
}

#[test]
fn should_serve_the_rollup_again_after_rollback() {
    // Arrange
    let fixture = fresh_rollup("rollup-copy-rollback");
    copy_row(&fixture);

    // Act
    fixture.execute("ROLLBACK").expect("rollback");
    let total = first_total(&fixture);

    // Assert
    assert_eq!(total, Value::Int64(2));
}
