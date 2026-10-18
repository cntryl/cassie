//! PostgreSQL 18.6 built-in vector catalog identities for #817 and #904.

use super::super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_discover_builtin_vector_catalog_identities() {
    // Arrange
    let fixture = sql_fixture("issue-817-builtin-vector-catalog-identities", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT oid, typname, typlen, typelem, typarray \
             FROM pg_catalog.pg_type \
             WHERE oid IN (22, 30) \
             ORDER BY oid",
        )
        .expect("read the finite PostgreSQL built-in vector rows");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![
            vec![
                Value::Int64(22),
                Value::String("int2vector".to_owned()),
                Value::Int64(-1),
                Value::Int64(21),
                Value::Int64(1_006),
            ],
            vec![
                Value::Int64(30),
                Value::String("oidvector".to_owned()),
                Value::Int64(-1),
                Value::Int64(26),
                Value::Int64(1_013),
            ],
        ]
    );
}

#[test]
fn should_preserve_builtin_vector_dimension_contracts() {
    // Arrange
    let fixture = sql_fixture("issue-817-builtin-vector-dimensions", &[]);
    fixture.cassie.startup().expect("startup");
    let sql = "SELECT array_dims('1 2 3'::int2vector), \
                      array_lower('1 2 3'::int2vector, 1)::text, \
                      ('1 2 3'::int2vector)[0]::text, \
                      ('1 2 3'::int2vector)[1]::text, \
                      ('1 2 3'::int2vector)[2]::text, \
                      array_dims('4 5'::oidvector), \
                      array_lower('4 5'::oidvector, 1)::text, \
                      ('4 5'::oidvector)[0]::text, \
                      ('4 5'::oidvector)[1]::text, \
                      array_dims(''::int2vector), \
                      array_length(''::int2vector, 1)::text, \
                      array_dims(''::oidvector), \
                      array_length(''::oidvector, 1)::text";

    // Act
    let selected = fixture
        .execute(sql)
        .expect("read zero-based vector elements and empty dimensions");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("[0:2]".to_owned()),
            Value::String("0".to_owned()),
            Value::String("1".to_owned()),
            Value::String("2".to_owned()),
            Value::String("3".to_owned()),
            Value::String("[0:1]".to_owned()),
            Value::String("0".to_owned()),
            Value::String("4".to_owned()),
            Value::String("5".to_owned()),
            Value::String("[0:-1]".to_owned()),
            Value::String("0".to_owned()),
            Value::String("[0:-1]".to_owned()),
            Value::String("0".to_owned()),
        ]]
    );
}
