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

#[test]
fn should_register_combine_function_metadata_for_sql_aggregate() {
    // Arrange
    let fixture = sql_fixture("issue-910-combine-function-metadata", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA issue_910_combine")
        .expect("create an isolated aggregate schema");
    fixture
        .execute(
            "CREATE AGGREGATE issue_910_combine.total(BIGINT) ( \
               SFUNC = pg_catalog.int8pl, STYPE = BIGINT, \
               COMBINEFUNC = pg_catalog.int8pl, INITCOND = '5', PARALLEL = SAFE)",
        )
        .expect("register the SQL aggregate's combine function and parallel safety");

    // Act
    let result = fixture
        .execute(
            "SELECT issue_910_combine.total(value) \
             FROM (VALUES (10::BIGINT), (20::BIGINT)) AS inputs(value)",
        )
        .expect("evaluate the custom aggregate over bigint inputs");
    let metadata = fixture
        .execute(
            "SELECT aggregate.agginitval, aggregate.aggcombinefn::REGPROCEDURE::TEXT, \
                    procedure.proparallel::TEXT \
             FROM pg_catalog.pg_aggregate AS aggregate \
             JOIN pg_catalog.pg_proc AS procedure \
               ON procedure.oid = aggregate.aggfnoid \
             WHERE procedure.oid = \
               'issue_910_combine.total(bigint)'::REGPROCEDURE",
        )
        .expect("read the aggregate's combine routine and parallel metadata");

    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(35)]]);
    assert_eq!(
        metadata.rows(),
        &[vec![
            Value::String("5".to_owned()),
            Value::String("int8pl(bigint,bigint)".to_owned()),
            Value::String("s".to_owned()),
        ]]
    );
}

#[test]
fn should_execute_sql_defined_moving_aggregate_with_an_inverse_transition() {
    // Arrange
    let fixture = sql_fixture("issue-910-moving-sql-aggregate", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA issue_910_moving")
        .expect("create an isolated aggregate schema");
    fixture
        .execute(
            "CREATE AGGREGATE issue_910_moving.moving_sum(INTEGER) (\
               SFUNC = pg_catalog.int4pl, STYPE = INTEGER, \
               MSFUNC = pg_catalog.int4pl, MINVFUNC = pg_catalog.int4mi, \
               MSTYPE = INTEGER, INITCOND = '0', MINITCOND = '0')",
        )
        .expect("create a moving aggregate with an inverse transition");
    let sql = "SELECT issue_910_moving.moving_sum(value) OVER (\
                 ORDER BY value ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) \
               FROM (VALUES (1), (2), (3)) AS inputs(value)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("execute the moving transition and inverse transition")
            .rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(3)],
            vec![Value::Int64(5)],
        ]
    );
}
