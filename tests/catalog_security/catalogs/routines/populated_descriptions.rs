//! Populated `pg_description` rows from the PostgreSQL 18.6 catalog for #817.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_return_the_builtin_now_description_by_function_identity() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-description-now", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT classoid, objoid, objsubid, description \
             FROM pg_catalog.pg_description \
             WHERE classoid = 'pg_catalog.pg_proc'::regclass \
               AND objoid = 'pg_catalog.now()'::regprocedure::oid \
               AND objsubid = 0",
        )
        .expect("read the built-in function description by catalog identity");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![26, 26, 23, 25]
    );
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::Int64(1_255),
            Value::Int64(1_299),
            Value::Int64(0),
            Value::String("current transaction time".to_owned()),
        ]]
    );
}
