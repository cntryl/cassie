use super::*;

#[test]
fn should_run_sql_support_functions_as_aggregate_state_handlers() {
    // Arrange
    let fixture = sql_fixture("issue-910-sql-support-functions", &[]);
    fixture.cassie.startup().expect("startup");
    for statement in [
        "CREATE SCHEMA issue_910_support_functions",
        "CREATE FUNCTION issue_910_support_functions.sum_transition( \
           state_value BIGINT, input_value INTEGER) RETURNS BIGINT \
           LANGUAGE SQL IMMUTABLE STRICT AS $$ SELECT $1 + $2 $$",
        "CREATE FUNCTION issue_910_support_functions.render_total( \
           state_value BIGINT) RETURNS TEXT LANGUAGE SQL IMMUTABLE \
           AS $$ SELECT 'total=' || $1::TEXT $$",
        "CREATE AGGREGATE issue_910_support_functions.total_with_label(INTEGER) ( \
           SFUNC = issue_910_support_functions.sum_transition, \
           STYPE = BIGINT, \
           FINALFUNC = issue_910_support_functions.render_total, \
           INITCOND = '10')",
    ] {
        fixture.execute(statement).unwrap_or_else(|error| {
            panic!("aggregate support fixture failed for {statement}: {error}")
        });
    }
    let nonempty_sql = "SELECT \
        issue_910_support_functions.total_with_label(value), \
        pg_typeof(issue_910_support_functions.total_with_label(value))::REGTYPE::TEXT, \
        pg_typeof(issue_910_support_functions.total_with_label(value))::OID::BIGINT \
        FROM (VALUES (2), (4)) AS inputs(value)";
    let empty_sql = "SELECT \
        issue_910_support_functions.total_with_label(value), \
        pg_typeof(issue_910_support_functions.total_with_label(value))::REGTYPE::TEXT, \
        pg_typeof(issue_910_support_functions.total_with_label(value))::OID::BIGINT \
        FROM (VALUES (2), (4)) AS inputs(value) WHERE FALSE";
    let metadata_sql = "SELECT \
        procedure.prokind::TEXT, procedure.prorettype::BIGINT, \
        aggregate.aggtranstype::BIGINT, aggregate.agginitval, aggregate.aggkind::TEXT, \
        aggregate.aggtransfn = \
          'issue_910_support_functions.sum_transition(bigint,integer)'::REGPROCEDURE::OID, \
        aggregate.aggfinalfn = \
          'issue_910_support_functions.render_total(bigint)'::REGPROCEDURE::OID \
        FROM pg_catalog.pg_proc AS procedure \
        JOIN pg_catalog.pg_aggregate AS aggregate ON aggregate.aggfnoid = procedure.oid \
        WHERE procedure.oid = \
          'issue_910_support_functions.total_with_label(integer)'::REGPROCEDURE";

    // Act
    let nonempty = fixture.execute(nonempty_sql);
    let empty = fixture.execute(empty_sql);
    let metadata = fixture.execute(metadata_sql);

    // Assert
    assert_eq!(
        nonempty
            .expect("run transition and final SQL functions")
            .rows(),
        &[vec![
            Value::String("total=16".to_owned()),
            Value::String("text".to_owned()),
            Value::Int64(25),
        ]]
    );
    assert_eq!(
        empty
            .expect("apply the final function to the initial state with no inputs")
            .rows(),
        &[vec![
            Value::String("total=10".to_owned()),
            Value::String("text".to_owned()),
            Value::Int64(25),
        ]]
    );
    assert_eq!(
        metadata
            .expect("inspect the SQL-defined aggregate catalog row")
            .rows(),
        &[vec![
            Value::String("a".to_owned()),
            Value::Int64(25),
            Value::Int64(20),
            Value::String("10".to_owned()),
            Value::String("n".to_owned()),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}
