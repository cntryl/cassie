//! PostgreSQL 18 SQL-defined operator behavior tracked by #912.

#[path = "overload_resolution.rs"]
mod overload_resolution;

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_execute_sql_defined_operator_with_its_declared_function() {
    // Arrange
    let fixture = sql_fixture("issue-912-sql-defined-operator", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE OPERATOR ## (LEFTARG = INTEGER, RIGHTARG = INTEGER, FUNCTION = pg_catalog.int4pl)",
        )
        .expect("CREATE OPERATOR should register its declared SQL function");

    // Act
    let result = fixture.execute("SELECT 3 ## 4");

    // Assert
    assert_eq!(
        result.expect("SQL-defined operator").rows(),
        &[vec![Value::Int64(7)]]
    );
}

#[test]
fn should_execute_a_sql_defined_unary_operator() {
    // Arrange
    let fixture = sql_fixture("issue-912-sql-defined-unary-operator", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE OPERATOR #@# (RIGHTARG = INTEGER, FUNCTION = pg_catalog.int4um)")
        .expect("create a prefix operator with its unary support function");

    // Act
    let result = fixture.execute("SELECT #@# 5");

    // Assert
    assert_eq!(
        result.expect("SQL-defined unary operator").rows(),
        &[vec![Value::Int64(-5)]]
    );
}

#[test]
fn should_register_custom_btree_operator_family_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-912-custom-btree-operator-family", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE OPERATOR FAMILY issue_912_int4_family USING btree")
        .expect("create a named B-tree operator family");
    fixture
        .execute(
            "CREATE OPERATOR CLASS issue_912_int4_ops FOR TYPE integer USING btree \
             FAMILY issue_912_int4_family AS \
               OPERATOR 1 < (integer, integer), \
               OPERATOR 3 = (integer, integer), \
               OPERATOR 5 > (integer, integer), \
               FUNCTION 1 btint4cmp(integer, integer)",
        )
        .expect("create an operator class with strategies and a support function");
    let sql = "SELECT amop.amopstrategy::BIGINT, op.oprname, \
                      left_type.typname, right_type.typname \
               FROM pg_catalog.pg_amop AS amop \
               JOIN pg_catalog.pg_operator AS op ON op.oid = amop.amopopr \
               JOIN pg_catalog.pg_type AS left_type ON left_type.oid = op.oprleft \
               JOIN pg_catalog.pg_type AS right_type ON right_type.oid = op.oprright \
               JOIN pg_catalog.pg_opfamily AS family ON family.oid = amop.amopfamily \
               WHERE family.opfname = 'issue_912_int4_family' \
               ORDER BY amop.amopstrategy";

    // Act
    let operators = fixture
        .execute(sql)
        .expect("inspect the registered strategy operators");
    let support = fixture
        .execute(
            "SELECT amproc.amprocnum::BIGINT, proc.proname \
             FROM pg_catalog.pg_amproc AS amproc \
             JOIN pg_catalog.pg_proc AS proc ON proc.oid = amproc.amproc \
             JOIN pg_catalog.pg_opfamily AS family ON family.oid = amproc.amprocfamily \
             WHERE family.opfname = 'issue_912_int4_family'",
        )
        .expect("inspect the registered support function");

    // Assert
    assert_eq!(
        operators.rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("<".to_owned()),
                Value::String("int4".to_owned()),
                Value::String("int4".to_owned()),
            ],
            vec![
                Value::Int64(3),
                Value::String("=".to_owned()),
                Value::String("int4".to_owned()),
                Value::String("int4".to_owned()),
            ],
            vec![
                Value::Int64(5),
                Value::String(">".to_owned()),
                Value::String("int4".to_owned()),
                Value::String("int4".to_owned()),
            ],
        ]
    );
    assert_eq!(
        support.rows(),
        &[vec![Value::Int64(1), Value::String("btint4cmp".to_owned())]]
    );
}

#[test]
fn should_publish_custom_operator_commutator_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-912-operator-commutator-catalog", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA issue_912_operator_catalog")
        .expect("create an isolated operator schema");
    fixture
        .execute("SET search_path TO issue_912_operator_catalog, public")
        .expect("make the custom operator schema visible");
    fixture
        .execute(
            "CREATE OPERATOR ## (LEFTARG = INTEGER, RIGHTARG = INTEGER, \
             FUNCTION = pg_catalog.int4eq, \
             COMMUTATOR = OPERATOR(issue_912_operator_catalog.##))",
        )
        .expect("create an equality operator with a self commutator");

    // Act
    let metadata = fixture
        .execute(
            "SELECT op.oprkind::text, op.oprresult = 'boolean'::regtype::oid, \
                    op.oprcom = op.oid, op.oprcode::regproc::text \
             FROM pg_catalog.pg_operator AS op \
             WHERE op.oprnamespace = 'issue_912_operator_catalog'::regnamespace \
               AND op.oprname = '##'",
        )
        .expect("read the custom operator signature and commutator");

    // Assert
    assert_eq!(
        metadata.rows(),
        &[vec![
            Value::String("b".to_owned()),
            Value::Bool(true),
            Value::Bool(true),
            Value::String("int4eq".to_owned()),
        ]]
    );
}
