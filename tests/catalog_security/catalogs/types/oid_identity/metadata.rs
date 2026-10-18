//! PostgreSQL 18.6 OID identity and database-scope contracts for #818.

use super::super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_report_postgresql_object_identifier_type_oids() {
    // Arrange
    let fixture = sql_fixture("issue-818-object-identifier-type-oids", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT typname, oid FROM pg_catalog.pg_type \
             WHERE typname IN ('oid', 'regproc', 'regoper', 'regoperator', \
                               'regclass', 'regtype', 'regrole', 'regnamespace', \
                               'regconfig', 'regdictionary', 'regprocedure') \
             ORDER BY oid",
        )
        .expect("read the PostgreSQL object-identifier type identities");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![
            vec![Value::String("regproc".to_owned()), Value::Int64(24)],
            vec![Value::String("oid".to_owned()), Value::Int64(26)],
            vec![
                Value::String("regprocedure".to_owned()),
                Value::Int64(2_202)
            ],
            vec![Value::String("regoper".to_owned()), Value::Int64(2_203)],
            vec![Value::String("regoperator".to_owned()), Value::Int64(2_204)],
            vec![Value::String("regclass".to_owned()), Value::Int64(2_205)],
            vec![Value::String("regtype".to_owned()), Value::Int64(2_206)],
            vec![Value::String("regconfig".to_owned()), Value::Int64(3_734)],
            vec![
                Value::String("regdictionary".to_owned()),
                Value::Int64(3_769)
            ],
            vec![
                Value::String("regnamespace".to_owned()),
                Value::Int64(4_089)
            ],
            vec![Value::String("regrole".to_owned()), Value::Int64(4_096)],
        ]
    );
}

#[test]
fn should_round_trip_the_unsigned_oid_boundary_across_integer_types() {
    // Arrange
    let fixture = sql_fixture("issue-818-oid-unsigned-boundary", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_typeof(4294967295::oid)::oid, \
                    4294967295::oid::text, \
                    4294967295::oid::bigint, \
                    4294967295::bigint::oid = 4294967295::oid, \
                    2147483648::oid > 2147483647::oid, \
                    42::oid = 42::integer, \
                    42::oid = 42::bigint",
        )
        .expect("round-trip the upper OID boundary and compare integer identities");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Int64(26),
            Value::String("4294967295".to_owned()),
            Value::Int64(4_294_967_295),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}
