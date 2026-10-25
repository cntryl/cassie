//! PostgreSQL 18.6 `information_schema` descriptor and row contracts for #817.

use crate::support_sql_fixture::sql_fixture;
use cassie::executor::QueryResult;

fn assert_descriptor(result: &QueryResult, expected: &[(&str, i64, i16, i32, bool)]) {
    let actual = result
        .columns()
        .iter()
        .map(|column| {
            (
                column.name.clone(),
                column.type_oid,
                column.typlen,
                column.atttypmod,
                column.nullable,
            )
        })
        .collect::<Vec<_>>();
    let expected = expected
        .iter()
        .map(|(name, type_oid, typlen, atttypmod, nullable)| {
            (
                (*name).to_owned(),
                *type_oid,
                *typlen,
                *atttypmod,
                *nullable,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn should_describe_pg_constraint_catalog_columns_with_postgresql_types() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-constraint-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT oid, conname, contype, conenforced, convalidated, conrelid \
             FROM pg_catalog.pg_constraint LIMIT 0",
        )
        .expect("describe PostgreSQL 18 constraint catalog fields");

    // Assert
    assert_descriptor(
        &selected,
        &[
            ("oid", 23, 4, -1, true),
            ("conname", 25, -1, -1, true),
            ("contype", 25, -1, -1, true),
            ("conenforced", 16, 1, -1, true),
            ("convalidated", 16, 1, -1, true),
            ("conrelid", 25, -1, -1, true),
        ],
    );
}
