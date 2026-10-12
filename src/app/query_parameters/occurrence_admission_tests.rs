//! Selected parameter admission must precede cache and EXPLAIN shortcuts.

use std::path::PathBuf;
use std::time::Instant;

use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits, ExecutionResultCacheEnabled};
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::{ExecutionMode, QueryExecutionControls};
use crate::sql::{checked_parameter_types_in_scope, parse_statement, PrivateParameterTypeInput};
use crate::types::Value;

const BOOLEAN_SQL: &str = "SELECT $1 AS flag WHERE $1";
const WHERE_ERROR: &str = "argument of WHERE must be type boolean, not type text";
const AMPLE_BUDGET: usize = 8 * 1024 * 1024;

struct Fixture {
    cassie: Option<Cassie>,
    path: PathBuf,
}

impl Fixture {
    fn new(cache_enabled: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-parameter-occurrence-callers-{}",
            uuid::Uuid::new_v4()
        ));
        let mut config = CassieRuntimeConfig::default();
        config.limits.query_memory_budget_bytes = AMPLE_BUDGET;
        config.limits.execution_result_cache_enabled = if cache_enabled {
            ExecutionResultCacheEnabled::enabled()
        } else {
            ExecutionResultCacheEnabled::disabled()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("isolated store");
        cassie.startup().expect("startup");
        Self {
            cassie: Some(cassie),
            path,
        }
    }

    fn cassie(&self) -> &Cassie {
        self.cassie.as_ref().expect("live fixture")
    }

    fn execute(
        &self,
        session: &CassieSession,
        sql: &str,
        value: Value,
        declared: &[i32],
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let parsed = parse_statement(sql).expect("fixture SQL");
        let fingerprint = crate::runtime::sql_fingerprint(&parsed);
        self.cassie()
            .execute_parsed_statement_core_with_parameter_oids(
                session,
                parsed,
                fingerprint,
                (vec![value], declared),
                ExecutionMode::SimpleQuery,
                controls,
            )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.cassie.take());
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path).expect("remove retired caller fixture");
        }
    }
}

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn planner_failure<T>(outcome: Result<T, CassieError>) -> Option<(bool, String)> {
    match outcome {
        Ok(_) => None,
        Err(CassieError::Planner(message)) => Some((true, message)),
        Err(error) => Some((false, error.to_string())),
    }
}

fn repeated_parameter_sql() -> String {
    let projection = (0..512)
        .map(|index| format!("$1 AS p{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("SELECT {projection}")
}

type BooleanOutcome = (Vec<Vec<Value>>, i64);

fn boolean_outcome(
    fixture: &Fixture,
    session: &CassieSession,
    value: Value,
    declared: &[i32],
    controls: &QueryExecutionControls,
) -> Result<BooleanOutcome, CassieError> {
    let result = fixture.execute(session, BOOLEAN_SQL, value, declared, controls)?;
    let outcome = (result.rows().to_vec(), result.columns()[0].type_oid);
    drop(result);
    Ok(outcome)
}

fn cache_hits(fixture: &Fixture) -> (u64, u64) {
    let metrics = fixture.cassie().runtime.snapshot();
    (metrics.plan_cache.hits, metrics.execution_result_cache.hits)
}

fn cached_null_outcomes(
    fixture: &Fixture,
    session: &CassieSession,
    null_controls: &QueryExecutionControls,
    typed_controls: &QueryExecutionControls,
) -> (Vec<BooleanOutcome>, [(u64, u64); 3]) {
    let first = boolean_outcome(fixture, session, Value::Null, &[], null_controls)
        .expect("untyped embedded NULL keeps contextual compatibility");
    let before_repeat = cache_hits(fixture);
    let repeated = boolean_outcome(fixture, session, Value::Null, &[], null_controls)
        .expect("repeat untyped NULL through actual caches");
    let after_repeat = cache_hits(fixture);
    let typed = boolean_outcome(fixture, session, Value::Null, &[16], typed_controls)
        .expect("declared Boolean NULL through warmed caches");
    let repeated_typed = boolean_outcome(fixture, session, Value::Null, &[16], typed_controls)
        .expect("repeat declared Boolean NULL through actual caches");
    let after_typed = cache_hits(fixture);
    (
        vec![first, repeated, typed, repeated_typed],
        [before_repeat, after_repeat, after_typed],
    )
}

fn describe_unknown_failures(
    fixture: &Fixture,
    session: &CassieSession,
) -> [Option<(bool, String)>; 2] {
    [0, 705].map(|oid| {
        let parsed = parse_statement(BOOLEAN_SQL).expect("same SQL for wire UNKNOWN description");
        let fingerprint = crate::runtime::sql_fingerprint(&parsed);
        planner_failure(fixture.cassie().describe_parsed_statement_for_session(
            session,
            parsed,
            fingerprint,
            &[oid],
        ))
    })
}

#[test]
fn should_preserve_embedded_parameter_provenance() {
    // Arrange
    let fixture = Fixture::new(true);
    let session = fixture.cassie().create_session("root", None);
    let accepted_controls = controls(AMPLE_BUDGET);
    let null_controls = controls(AMPLE_BUDGET);
    let typed_null_controls = controls(AMPLE_BUDGET);
    let text_controls = controls(AMPLE_BUDGET);
    let text_null_controls = controls(AMPLE_BUDGET);

    // Act
    let boolean = boolean_outcome(
        &fixture,
        &session,
        Value::Bool(true),
        &[],
        &accepted_controls,
    )
    .expect("concrete embedded Boolean");
    let (nulls, [before_repeat, after_repeat, after_typed]) =
        cached_null_outcomes(&fixture, &session, &null_controls, &typed_null_controls);
    let before_rejections = cache_hits(&fixture);
    let text = planner_failure(fixture.execute(
        &session,
        BOOLEAN_SQL,
        Value::String("true".to_owned()),
        &[],
        &text_controls,
    ));
    let text_null = planner_failure(fixture.execute(
        &session,
        BOOLEAN_SQL,
        Value::Null,
        &[25],
        &text_null_controls,
    ));
    let unknown = describe_unknown_failures(&fixture, &session);
    let after_rejections = cache_hits(&fixture);
    let recovered = boolean_outcome(
        &fixture,
        &session,
        Value::Bool(true),
        &[],
        &accepted_controls,
    )
    .expect("concrete Boolean recovers through its existing cache entry");
    let after_recovery = cache_hits(&fixture);
    drop((session, fixture));

    // Assert
    assert_eq!(boolean, (vec![vec![Value::Bool(true)]], 16));
    assert_eq!(recovered, boolean);
    for (rows, oid) in nulls {
        assert_eq!(rows, Vec::<Vec<Value>>::new());
        assert_eq!(oid, 16);
    }
    assert_eq!(after_repeat, (before_repeat.0 + 1, before_repeat.1 + 1));
    assert_eq!(after_typed, (after_repeat.0 + 2, after_repeat.1 + 2));
    assert_eq!(text, Some((true, WHERE_ERROR.to_owned())));
    assert_eq!(text_null, Some((true, WHERE_ERROR.to_owned())));
    for error in unknown {
        assert_eq!(
            error,
            Some((
                true,
                "inconsistent types deduced for parameter $1".to_owned()
            ))
        );
    }
    assert_eq!(after_rejections, before_rejections);
    assert_eq!(
        after_recovery,
        (after_rejections.0 + 1, after_rejections.1 + 1)
    );
    for owner in [
        accepted_controls,
        null_controls,
        typed_null_controls,
        text_controls,
        text_null_controls,
    ] {
        assert_eq!(owner.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_parameter_metadata_before_warm_cache_hits() {
    // Arrange
    let fixture = Fixture::new(true);
    let session = fixture.cassie().create_session("root", None);
    let sql = repeated_parameter_sql();
    let parsed = parse_statement(&sql).expect("512 valid projection columns");
    let values = [Value::Bool(true)];
    let metadata_controls = controls(AMPLE_BUDGET);
    let admitted = checked_parameter_types_in_scope(
        &parsed,
        PrivateParameterTypeInput::Embedded {
            effective: &[16],
            values: &values,
        },
        &fixture.cassie().catalog,
        &metadata_controls,
        None,
    )
    .expect("ample occurrence admission");
    assert_eq!(admitted.types(), [16]);
    drop(admitted);
    let denied_budget = metadata_controls
        .peak_query_memory_bytes()
        .checked_sub(1)
        .expect("nonzero required occurrence capacity");
    let warm_controls = controls(AMPLE_BUDGET);
    let first = fixture
        .execute(&session, &sql, Value::Bool(true), &[16], &warm_controls)
        .expect("warm exact SQL and parameter shape");
    let first_rows = first.rows().to_vec();
    drop(first);
    let before_hit = fixture.cassie().runtime.snapshot();
    let second = fixture
        .execute(&session, &sql, Value::Bool(true), &[16], &warm_controls)
        .expect("actual cached execution");
    let second_rows = second.rows().to_vec();
    drop(second);
    let before_denial = fixture.cassie().runtime.snapshot();
    let denied_controls = controls(denied_budget);

    // Act
    let denied = fixture.execute(&session, &sql, Value::Bool(true), &[16], &denied_controls);
    let denied_as_memory = matches!(&denied, Err(CassieError::ResourceLimit(_)));
    let denial_error = denied.as_ref().err().map(ToString::to_string);
    drop(denied);
    let after_denial = fixture.cassie().runtime.snapshot();
    let recovered = fixture
        .execute(&session, &sql, Value::Bool(true), &[16], &warm_controls)
        .expect("ample cached execution after denial");
    let recovered_rows = recovered.rows().to_vec();
    drop(recovered);
    drop((session, fixture));

    // Assert
    let expected = vec![vec![Value::Bool(true); 512]];
    assert_eq!(first_rows, expected);
    assert_eq!(second_rows, expected);
    assert_eq!(recovered_rows, expected);
    assert_eq!(
        before_denial.plan_cache.hits,
        before_hit.plan_cache.hits + 1
    );
    assert_eq!(
        before_denial.execution_result_cache.hits,
        before_hit.execution_result_cache.hits + 1
    );
    assert!(denied_as_memory, "metadata admission: {denial_error:?}");
    assert_eq!(after_denial.plan_cache.hits, before_denial.plan_cache.hits);
    assert_eq!(
        after_denial.execution_result_cache.hits,
        before_denial.execution_result_cache.hits
    );
    assert_eq!(denied_controls.current_query_memory_bytes(), 0);
    assert_eq!(metadata_controls.current_query_memory_bytes(), 0);
    assert_eq!(warm_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_parameter_types_before_explain_describe() {
    // Arrange
    let fixture = Fixture::new(false);
    let session = fixture.cassie().create_session("root", None);
    let sql = format!("EXPLAIN {BOOLEAN_SQL}");
    let parsed = parse_statement(&sql).expect("EXPLAIN fixture");
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);

    // Act
    let text = planner_failure(fixture.cassie().describe_parsed_statement_for_session(
        &session,
        parsed.clone(),
        fingerprint,
        &[25],
    ));
    let boolean = fixture.cassie().describe_parsed_statement_for_session(
        &session,
        parsed,
        fingerprint,
        &[16],
    );
    drop((session, fixture));

    // Assert
    assert_eq!(text, Some((true, WHERE_ERROR.to_owned())));
    assert_eq!(
        boolean.expect("valid Boolean EXPLAIN description"),
        vec![ColumnMeta::text("QUERY PLAN")]
    );
}

#[test]
fn should_validate_parameter_types_before_explain_execution() {
    // Arrange
    let fixture = Fixture::new(false);
    let session = fixture.cassie().create_session("root", None);
    let text_controls = controls(AMPLE_BUDGET);
    let sql_text_controls = controls(AMPLE_BUDGET);
    let boolean_controls = controls(AMPLE_BUDGET);
    let sql_boolean_controls = controls(AMPLE_BUDGET);

    // Act
    let text = planner_failure(fixture.cassie().explain_sql_core(
        &session,
        BOOLEAN_SQL,
        vec![Value::String("true".to_owned())],
        &text_controls,
    ));
    let sql = format!("EXPLAIN {BOOLEAN_SQL}");
    let sql_text = planner_failure(fixture.cassie().execute_sql_with_controls(
        &session,
        &sql,
        vec![Value::String("true".to_owned())],
        ExecutionMode::SimpleQuery,
        &sql_text_controls,
    ));
    let boolean = fixture
        .cassie()
        .explain_sql_core(
            &session,
            BOOLEAN_SQL,
            vec![Value::Bool(true)],
            &boolean_controls,
        )
        .expect("valid structured Boolean EXPLAIN");
    let boolean_columns = boolean.result().columns().to_vec();
    let boolean_live = boolean_controls.current_query_memory_bytes();
    drop(boolean);
    let sql_boolean = fixture
        .cassie()
        .execute_sql_with_controls(
            &session,
            &sql,
            vec![Value::Bool(true)],
            ExecutionMode::SimpleQuery,
            &sql_boolean_controls,
        )
        .expect("valid SQL Boolean EXPLAIN");
    let sql_boolean_columns = sql_boolean.columns().to_vec();
    let sql_boolean_live = sql_boolean_controls.current_query_memory_bytes();
    drop(sql_boolean);
    drop((session, fixture));

    // Assert
    assert_eq!(text, Some((true, WHERE_ERROR.to_owned())));
    assert_eq!(sql_text, Some((true, WHERE_ERROR.to_owned())));
    assert_eq!(boolean_columns, vec![ColumnMeta::text("QUERY PLAN")]);
    assert_eq!(sql_boolean_columns, vec![ColumnMeta::text("QUERY PLAN")]);
    assert!(boolean_live > 0);
    assert!(sql_boolean_live > 0);
    for owner in [
        text_controls,
        sql_text_controls,
        boolean_controls,
        sql_boolean_controls,
    ] {
        assert_eq!(owner.current_query_memory_bytes(), 0);
    }
}
