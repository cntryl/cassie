//! SQL-defined aggregate serialization, deserialization and combine support (#910).

use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use std::fmt::Write as _;

use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config, SqlFixture};

const SERIALIZED_STATE_ROW_COUNT: usize = 4096;

fn parallel_serialized_array_fixture() -> SqlFixture {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let fixture = sql_fixture_with_config(
        "sql-defined-serialized-array-aggregate-parallel-table",
        &[],
        config,
    );
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA serialized_state")
        .expect("create an isolated schema for the aggregate");
    fixture
        .execute(
            "CREATE AGGREGATE serialized_state.serialized_array_agg(ANYNONARRAY) ( \
                 SFUNC = pg_catalog.array_agg_transfn, \
                 STYPE = INTERNAL, \
                 FINALFUNC = pg_catalog.array_agg_finalfn, \
                 FINALFUNC_EXTRA, \
                 COMBINEFUNC = pg_catalog.array_agg_combine, \
                 SERIALFUNC = pg_catalog.array_agg_serialize, \
                 DESERIALFUNC = pg_catalog.array_agg_deserialize, \
                 PARALLEL = SAFE \
             )",
        )
        .expect("register the built-in internal-state support functions");
    fixture
        .execute("CREATE TABLE serialized_state.inputs (value INTEGER)")
        .expect("create a real table as the partial aggregation input");

    let mut insert = String::from("INSERT INTO serialized_state.inputs(value) VALUES ");
    for value in 1..=SERIALIZED_STATE_ROW_COUNT {
        if value > 1 {
            insert.push(',');
        }
        write!(&mut insert, "({value})").expect("append input value to SQL");
    }
    fixture
        .execute(&insert)
        .expect("insert enough rows for multiple aggregation workers");
    fixture
}

fn assert_parallel_aggregation_metrics(metrics: &serde_json::Value) {
    assert!(
        metrics["parallel_aggregation"]["aggregations"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "EXPLAIN ANALYZE and the value query should both run partial aggregation: {metrics}"
    );
    assert!(
        metrics["parallel_aggregation"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 4,
        "multiple workers should contribute to serialized states: {metrics}"
    );
    assert!(
        metrics["parallel_aggregation"]["rows"]
            .as_u64()
            .unwrap_or(0)
            >= u64::try_from(SERIALIZED_STATE_ROW_COUNT * 2).expect("test row count fits u64"),
        "both executions should report the rows consumed by partial aggregation: {metrics}"
    );
}

#[test]
fn should_register_serialization_support_metadata_given_an_internal_state_aggregate_when_it_is_created(
) {
    // Arrange
    let fixture = sql_fixture("sql-defined-serialized-array-aggregate", &[]);
    fixture.cassie.startup().expect("startup");
    let create_aggregate = fixture.execute(
        "CREATE AGGREGATE serialized_array_agg(ANYNONARRAY) ( \
             SFUNC = pg_catalog.array_agg_transfn, \
             STYPE = INTERNAL, \
             FINALFUNC = pg_catalog.array_agg_finalfn, \
             FINALFUNC_EXTRA, \
             COMBINEFUNC = pg_catalog.array_agg_combine, \
             SERIALFUNC = pg_catalog.array_agg_serialize, \
             DESERIALFUNC = pg_catalog.array_agg_deserialize, \
             PARALLEL = SAFE \
         )",
    );
    let query = fixture.execute(
        "SELECT serialized_array_agg(value)::TEXT \
         FROM (VALUES (1::INT4), (2::INT4), (NULL::INT4)) AS inputs(value)",
    );
    let metadata = fixture.execute(
        "SELECT aggregate.aggcombinefn = \
                   'pg_catalog.array_agg_combine(internal,internal)'::REGPROCEDURE::OID, \
                aggregate.aggserialfn = \
                   'pg_catalog.array_agg_serialize(internal)'::REGPROCEDURE::OID, \
                aggregate.aggdeserialfn = \
                   'pg_catalog.array_agg_deserialize(internal,internal)'::REGPROCEDURE::OID \
         FROM pg_catalog.pg_aggregate AS aggregate \
         JOIN pg_catalog.pg_proc AS procedure ON procedure.oid = aggregate.aggfnoid \
         WHERE procedure.proname = 'serialized_array_agg'",
    );

    // Act
    let create_aggregate = create_aggregate.expect("register serialization and combine support");
    let query = query.expect("combine and serialize partial array aggregate state");
    let metadata = metadata.expect("read aggregate state support functions");

    // Assert
    assert_eq!(create_aggregate.command(), "CREATE AGGREGATE");
    assert_eq!(
        query.columns()[0].type_oid,
        25,
        "array output is explicitly rendered as text"
    );
    assert_eq!(
        query.rows(),
        &[vec![Value::String("{1,2,NULL}".to_owned())]]
    );
    assert_eq!(
        metadata.rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_merge_partial_aggregate_states_given_serialization_support_when_parallel_table_aggregation_executes(
) {
    // Arrange
    let fixture = parallel_serialized_array_fixture();

    // Act
    let plan = fixture.execute(
        "EXPLAIN ANALYZE SELECT \
             serialized_state.serialized_array_agg(value)::TEXT \
         FROM serialized_state.inputs",
    );
    let result = fixture.execute(
        "SELECT serialized_state.serialized_array_agg(value)::TEXT \
         FROM serialized_state.inputs",
    );
    let metrics = fixture.cassie.metrics();

    // Assert
    let plan = plan.expect("explain and execute the partial aggregate");
    let plan = plan.rows()[0][0]
        .as_str()
        .expect("EXPLAIN ANALYZE returns a textual plan");
    assert!(
        plan.contains("aggregate_parallel=true"),
        "the plan must select parallel aggregation: {plan}"
    );
    assert!(
        plan.contains("parallel_aggregations_delta:"),
        "EXPLAIN ANALYZE reports partial aggregate metrics: {plan}"
    );
    assert!(
        plan.contains("parallel_aggregation_workers_delta:"),
        "EXPLAIN ANALYZE reports participating worker metrics: {plan}"
    );

    let result = result.expect("merge serialized states from the real input table");
    assert_eq!(result.columns()[0].type_oid, 25);
    let Value::String(encoded_array) = &result.rows()[0][0] else {
        panic!("serialized array aggregate should be rendered as text");
    };
    let values = encoded_array
        .strip_prefix('{')
        .and_then(|body| body.strip_suffix('}'))
        .expect("PostgreSQL array text has braces")
        .split(',')
        .map(|value| value.parse::<i64>().expect("array element is an integer"))
        .collect::<Vec<_>>();
    let mut sorted_values = values;
    sorted_values.sort_unstable();
    let expected_values = (1..=SERIALIZED_STATE_ROW_COUNT)
        .map(|value| i64::try_from(value).expect("test value fits BIGINT"))
        .collect::<Vec<_>>();
    assert_eq!(sorted_values, expected_values);

    assert_parallel_aggregation_metrics(&metrics);
}
