//! `age(timestamp, timestamp)` month-end borrowing pinned to PostgreSQL 18.6 (#815).

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_borrow_days_from_the_preceding_month_when_age_crosses_a_month_end() {
    // Arrange
    let fixture = sql_fixture("issue-815-age-month-end-borrow", &[]);
    let cases = [
        ("2024-03-31", "2024-02-29", "1 month 2 days"),
        ("2024-03-01", "2024-01-31", "1 month 1 day"),
        ("2023-03-01", "2023-01-31", "1 month 1 day"),
        ("2024-02-29", "2023-02-28", "1 year 1 day"),
    ];

    for (later, earlier, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT AGE(TIMESTAMP '{later}', TIMESTAMP '{earlier}') = INTERVAL '{expected}'"
        ));

        // Assert
        assert!(
            matches!(&result, Ok(r) if r.rows() == [vec![Value::Bool(true)]]),
            "age({later}, {earlier}) should equal {expected}"
        );
    }
}
