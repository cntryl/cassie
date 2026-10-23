//! PostgreSQL 18 multi-argument hypothetical-set aggregate contracts for #806.

use cassie::sql::{bind, parameter_type_oids_with_catalog, parse_statement};
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

#[test]
fn should_bind_hypothetical_parameters_from_each_ordering_key() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-hypothetical-multicolumn-parameters",
        &[
            "CREATE TABLE issue_806_hypothetical_parameter_values (score INT, label TEXT)",
            "INSERT INTO issue_806_hypothetical_parameter_values VALUES (1,'alpha'),(2,'beta'),(2,'gamma'),(4,'delta')",
        ],
    );
    let sql = "SELECT rank($1, $2) WITHIN GROUP (ORDER BY score, label) FROM issue_806_hypothetical_parameter_values";

    let parsed = parse_statement(sql).expect("hypothetical parameter syntax");
    let bound = bind(parsed, &fixture.cassie.catalog).expect("hypothetical aggregate binding");

    // Act
    let parameter_oids =
        parameter_type_oids_with_catalog(&bound.statement, &[705, 705], &fixture.cassie.catalog);

    // Assert
    assert_eq!(
        parameter_oids,
        vec![23, 25],
        "each direct parameter follows its matching order key type"
    );
}

#[test]
fn should_rank_hypotheticals_with_descending_nulls_first_ordering() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-hypothetical-descending-null-order",
        &[
            "CREATE TABLE issue_806_hypothetical_desc_values (n INT)",
            "INSERT INTO issue_806_hypothetical_desc_values VALUES (NULL),(3),(2),(2),(1)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT rank(2) WITHIN GROUP (ORDER BY n DESC NULLS FIRST), dense_rank(2) WITHIN GROUP (ORDER BY n DESC NULLS FIRST), percent_rank(2) WITHIN GROUP (ORDER BY n DESC NULLS FIRST), cume_dist(2) WITHIN GROUP (ORDER BY n DESC NULLS FIRST) FROM issue_806_hypothetical_desc_values",
    );

    // Assert
    let result = result.expect("hypothetical rows honor DESC and explicit NULL ordering");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(3),
            Value::Int64(3),
            Value::Float64(2.0 / 5.0),
            Value::Float64(5.0 / 6.0),
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
