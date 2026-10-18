use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_identify_builtin_routine_with_catalog_qualified_signature() {
    // Arrange
    let fixture = sql_fixture("issue-817-identify-builtin-routine", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture
        .execute(
            "SELECT type, schema, name, identity \
             FROM pg_catalog.pg_identify_object(\
               'pg_catalog.pg_proc'::regclass, \
               'pg_catalog.abs(integer)'::regprocedure, 0)",
        )
        .expect("identify an overloaded builtin function by its pg_proc identity");

    // Assert
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["type", "schema", "name", "identity"]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [25, 25, 25, 25]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("function".to_owned()),
            Value::String("pg_catalog".to_owned()),
            Value::Null,
            Value::String("pg_catalog.abs(integer)".to_owned()),
        ]]
    );
}
