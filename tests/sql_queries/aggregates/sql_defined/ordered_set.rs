//! Ordered-set signatures for SQL-defined aggregates (#910).

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_execute_ordered_set_semantics_given_a_sql_defined_aggregate_when_ordered_inputs_are_aggregated(
) {
    // Arrange
    let fixture = sql_fixture("sql-defined-ordered-set-aggregate", &[]);
    fixture.cassie.startup().expect("startup");
    let create_aggregate = fixture.execute(
        "CREATE AGGREGATE ordered_input_sum(INT4 ORDER BY INT4) ( \
             SFUNC = pg_catalog.int4pl, STYPE = INT4, INITCOND = '0' \
         )",
    );
    let control = fixture
        .execute("SELECT SUM(input_value) FROM (VALUES (3), (1), (2)) AS inputs(input_value)");
    let ordered = fixture.execute(
        "SELECT ordered_input_sum(0) WITHIN GROUP (ORDER BY input_value) \
         FROM (VALUES (3), (1), (2)) AS inputs(input_value)",
    );

    // Act
    let create_aggregate = create_aggregate.expect("create an ordered-set aggregate signature");
    let control = control.expect("ordinary aggregate control over the same input rows");
    let ordered = ordered.expect("run the SQL-defined ordered-set aggregate");

    // Assert
    assert_eq!(create_aggregate.command(), "CREATE AGGREGATE");
    assert_eq!(control.rows(), &[vec![Value::Int64(6)]]);
    assert_eq!(
        ordered
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23]
    );
    assert_eq!(ordered.rows(), &[vec![Value::Int64(6)]]);
}
