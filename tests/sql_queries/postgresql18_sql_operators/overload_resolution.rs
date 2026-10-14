use super::*;

#[test]
fn should_resolve_schema_scoped_operator_overloads_by_operand_type() {
    // Arrange
    let fixture = sql_fixture("issue-912-operator-overload-resolution", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA issue_912_operator_resolution")
        .expect("create an isolated operator schema");
    fixture
        .execute("SET search_path TO issue_912_operator_resolution, public")
        .expect("make the operator schema visible");
    for statement in [
        "CREATE OPERATOR ## (LEFTARG = INTEGER, RIGHTARG = INTEGER, \
         FUNCTION = pg_catalog.int4pl)",
        "CREATE OPERATOR ## (LEFTARG = BIGINT, RIGHTARG = BIGINT, \
         FUNCTION = pg_catalog.int8pl)",
    ] {
        fixture.execute(statement).unwrap_or_else(|error| {
            panic!("create overloaded SQL operator `{statement}`: {error}")
        });
    }

    // Act
    let expressions = fixture
        .execute(
            "SELECT 3 ## 4, 3::BIGINT ## 4::BIGINT, \
             3 OPERATOR(issue_912_operator_resolution.##) 4, \
             3::BIGINT OPERATOR(issue_912_operator_resolution.##) 4::BIGINT",
        )
        .expect("resolve both overloaded operators");
    let catalog = fixture
        .execute(
            "SELECT op.oid::BIGINT, op.oprleft::BIGINT, op.oprright::BIGINT, \
                    op.oprresult::BIGINT, namespace.nspname, procedure.proname, \
                    op.oprcode = procedure.oid \
             FROM pg_catalog.pg_operator AS op \
             JOIN pg_catalog.pg_namespace AS namespace \
               ON namespace.oid = op.oprnamespace \
             JOIN pg_catalog.pg_proc AS procedure ON procedure.oid = op.oprcode \
             WHERE namespace.nspname = 'issue_912_operator_resolution' \
               AND op.oprname = '##' \
             ORDER BY op.oprleft",
        )
        .expect("inspect operator overload identities");

    // Assert
    assert_eq!(
        expressions
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 20, 23, 20],
        "unqualified and schema-qualified overloads retain int4 and int8 result OIDs"
    );
    assert_eq!(
        expressions.rows(),
        &[vec![
            Value::Int64(7),
            Value::Int64(7),
            Value::Int64(7),
            Value::Int64(7),
        ]]
    );
    assert_eq!(catalog.rows().len(), 2, "one catalog row per signature");
    let bigint = &catalog.rows()[0];
    let int4 = &catalog.rows()[1];
    let Value::Int64(bigint_oid) = &bigint[0] else {
        panic!("bigint operator OID should be returned as BIGINT")
    };
    let Value::Int64(int4_oid) = &int4[0] else {
        panic!("int4 operator OID should be returned as BIGINT")
    };
    assert!(*bigint_oid > 0 && *int4_oid > 0);
    assert_ne!(bigint_oid, int4_oid, "overloads have distinct catalog OIDs");
    assert_eq!(
        &bigint[1..],
        &[
            Value::Int64(20),
            Value::Int64(20),
            Value::Int64(20),
            Value::String("issue_912_operator_resolution".to_owned()),
            Value::String("int8pl".to_owned()),
            Value::Bool(true),
        ]
    );
    assert_eq!(
        &int4[1..],
        &[
            Value::Int64(23),
            Value::Int64(23),
            Value::Int64(23),
            Value::String("issue_912_operator_resolution".to_owned()),
            Value::String("int4pl".to_owned()),
            Value::Bool(true),
        ]
    );
}
