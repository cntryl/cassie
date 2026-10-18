//! Cross-type B-tree operator-family behavior (#912).

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_cross_type_operator_laws_given_an_operator_family_when_indexed_queries_execute()
{
    // Arrange
    let fixture = sql_fixture("issue-912-cross-type-operator-family", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE OPERATOR FAMILY issue_912_int_cross_ops USING btree")
        .expect("create the cross-type B-tree family");
    fixture
        .execute(
            "CREATE OPERATOR CLASS issue_912_int4_cross_ops FOR TYPE integer USING btree \
             FAMILY issue_912_int_cross_ops AS \
               OPERATOR 1 < (integer, integer), \
               OPERATOR 3 = (integer, integer), \
               OPERATOR 5 > (integer, integer), \
               FUNCTION 1 pg_catalog.btint4cmp(integer, integer)",
        )
        .expect("define the int4 B-tree search strategies");
    fixture
        .execute(
            "CREATE OPERATOR === (LEFTARG = integer, RIGHTARG = bigint, \
             FUNCTION = pg_catalog.int48eq)",
        )
        .expect("define the cross-type equality operator");
    fixture
        .execute(
            "ALTER OPERATOR FAMILY issue_912_int_cross_ops USING btree ADD \
             OPERATOR 3 === (integer, bigint) FOR SEARCH, \
             FUNCTION 1 pg_catalog.btint48cmp(integer, bigint)",
        )
        .expect("add the cross-type search strategy and comparator");
    fixture
        .execute("CREATE TABLE issue_912_cross_type_rows (id INTEGER, value INTEGER)")
        .expect("create indexed rows");
    fixture
        .execute(
            "INSERT INTO issue_912_cross_type_rows \
             SELECT value, value FROM generate_series(1, 10000) AS source(value)",
        )
        .expect("populate the B-tree input relation");
    fixture
        .execute(
            "CREATE INDEX issue_912_cross_type_idx \
             ON issue_912_cross_type_rows USING btree (value issue_912_int4_cross_ops)",
        )
        .expect("create the index with the custom operator class");

    // Act
    let builtin_control = fixture
        .execute("SELECT 20 = CAST(20 AS BIGINT)")
        .expect("resolve PostgreSQL's built-in cross-type equality control");
    let indexed = fixture
        .execute(
            "SELECT id FROM issue_912_cross_type_rows \
             WHERE value === CAST(2000 AS BIGINT) ORDER BY id",
        )
        .expect("execute equality through the cross-type operator family");

    // Assert
    assert_eq!(builtin_control.rows(), &[vec![Value::Bool(true)]]);
    assert_eq!(indexed.rows(), &[vec![Value::Int64(2000)]]);
}
