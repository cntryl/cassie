//! PostgreSQL 18 money arithmetic, aggregate identity and overflow tracked by #908.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_money_arithmetic_precision() {
    // Arrange
    let fixture = sql_fixture("issue-908-money-exact-arithmetic", &[]);
    let sql = "SELECT (12.34::numeric::money + 0.01::numeric::money) = 12.35::numeric::money FROM (VALUES (12.34::numeric::money), (0.01::numeric::money)) AS cash_rows(amount)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("money arithmetic and aggregate identity")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}
