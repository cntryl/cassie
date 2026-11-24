//! PostgreSQL 18 exact NUMERIC behavior tracked by #812.

#[path = "../../types/numeric/high_precision_restart.rs"]
mod high_precision_restart;

use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

struct NumericDirectory(String);

impl Drop for NumericDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bool_rows(result: &cassie::executor::QueryResult) -> Vec<Vec<Value>> {
    result.rows().to_vec()
}

#[test]
fn should_sum_bigint_values_beyond_the_int64_range_as_numeric() {
    // Arrange
    let fixture = sql_fixture(
        "issue-812-bigint-sum-range",
        &[
            "CREATE TABLE issue_812_sum_values (value BIGINT)",
            "INSERT INTO issue_812_sum_values VALUES (9223372036854775807), (1)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT SUM(value) = CAST('9223372036854775808' AS NUMERIC) FROM issue_812_sum_values",
    );

    // Assert
    assert_eq!(
        result.expect("BIGINT sum uses exact numeric range").rows(),
        &[vec![Value::Bool(true)]]
    );
}

include!("../../types/numeric/numeric.rs");
