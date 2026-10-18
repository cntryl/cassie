//! PostgreSQL 18 multi-argument hypothetical-set aggregate contracts for #806.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_rank_multi_column_hypothetical_rows_lexicographically() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-hypothetical-multicolumn-ranking",
        &[
            "CREATE TABLE issue_806_hypothetical_multicolumn_values (score INT, label TEXT)",
            "INSERT INTO issue_806_hypothetical_multicolumn_values VALUES (1,'alpha'),(2,'beta'),(2,'gamma'),(4,'delta'),(NULL,'epsilon')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT rank(2, 'gamma') WITHIN GROUP (ORDER BY score, label), dense_rank(2, 'gamma') WITHIN GROUP (ORDER BY score, label), percent_rank(2, 'gamma') WITHIN GROUP (ORDER BY score, label), cume_dist(2, 'gamma') WITHIN GROUP (ORDER BY score, label) FROM issue_806_hypothetical_multicolumn_values",
    );

    // Assert
    let result = result.expect("hypothetical rows compare all direct arguments in order");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(3),
            Value::Int64(3),
            Value::Float64(0.4),
            Value::Float64(2.0 / 3.0),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 20, 701, 701]
    );
}
