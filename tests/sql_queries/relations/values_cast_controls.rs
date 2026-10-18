//! Resource ownership and cached parameter identity for typed VALUES.
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};
use cassie::types::Value;

fn physical(
    fixture: &SqlFixture,
    sql: &str,
) -> std::sync::Arc<cassie::planner::physical::PhysicalPlan> {
    let parsed = cassie::sql::parse_statement(sql).expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("plan");
    std::sync::Arc::new(cassie::planner::physical::build(logical))
}

#[test]
fn should_admit_character_padding_given_a_large_values_width_when_query_memory_is_limited() {
    // Arrange
    let fixture = sql_fixture("valuescasts-memory", &[]);
    let plan = physical(&fixture, "VALUES ('a'::CHAR(100000)),('b')");
    let mut limits = cassie::config::CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = 16384;
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls);
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_typed_values_given_cancelled_controls_when_execution_starts() {
    // Arrange
    let fixture = sql_fixture("valuescasts-cancel", &[]);
    let plan = physical(&fixture, "VALUES ('2024-02-29'::DATE),(NULL::DATE)");
    let limits = cassie::config::CassieRuntimeConfig::default().limits;
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls =
        QueryExecutionControls::with_cancellation(&limits, std::time::Instant::now(), cancellation);
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls);
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_typed_values_given_result_clones_when_the_original_owner_drops() {
    // Arrange
    let fixture = sql_fixture("valuescasts-owners", &[]);
    let plan = physical(&fixture, "VALUES ('2024-02-29'::DATE),(NULL::DATE)");
    let limits = cassie::config::CassieRuntimeConfig::default().limits;
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls)
        .expect("values");
    let retained = result.clone();
    drop(result);
    // Assert
    assert_eq!(retained.columns()[0].type_oid, 1082);
    assert_eq!(
        retained.rows(),
        &[vec![Value::String("2024-02-29".into())], vec![Value::Null]]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_rebind_timestamp_values_given_a_cached_statement_when_parameter_values_change() {
    // Arrange
    let fixture = sql_fixture("valuescasts-cache", &[]);
    let sql = "SELECT v::TEXT FROM (VALUES ($1::TIMESTAMP)) AS q(v)";
    // Act
    let first = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::String("2024-02-29 12:34:56.120000".into())],
    );
    let second = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::String("2000-01-01 00:00:00".into())],
    );
    // Assert
    assert_eq!(
        first.expect("first timestamp").rows(),
        &[vec![Value::String("2024-02-29 12:34:56.12".into())]]
    );
    assert_eq!(
        second.expect("second timestamp").rows(),
        &[vec![Value::String("2000-01-01 00:00:00".into())]]
    );
}

#[test]
fn should_admit_constant_character_projection_given_a_small_budget_when_padding_expands() {
    // Arrange
    let fixture = sql_fixture("valuescasts-constant-char-memory", &[]);
    let plan = physical(&fixture, "SELECT 'a'::CHAR(100000)");
    let mut limits = cassie::config::CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = 16384;
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls);
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_parameter_character_projection_given_changed_controls_when_padding_expands() {
    // Arrange
    let fixture = sql_fixture("valuescasts-parameter-char-memory", &[]);
    let plan = physical(&fixture, "SELECT $1::CHAR(100000)");
    let mut limits = cassie::config::CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = 16384;
    let denied = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    let normal_limits = cassie::config::CassieRuntimeConfig::default().limits;
    let admitted = QueryExecutionControls::from_limits(&normal_limits, std::time::Instant::now());
    // Act
    let rejected = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::String("a".into())],
        &denied,
    );
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::String("a".into())],
        &admitted,
    );
    // Assert
    assert!(matches!(
        rejected.map_err(CassieError::from),
        Err(CassieError::ResourceLimit(_))
    ));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    let result = result.expect("larger budget admits the same plan");
    let Value::String(text) = &result.rows()[0][0] else {
        panic!("CHAR string");
    };
    assert_eq!(text.len(), 100_000);
    assert!(text.starts_with('a'));
    assert!(text[1..].bytes().all(|byte| byte == b' '));
    drop(result);
    assert_eq!(admitted.current_query_memory_bytes(), 0);
}
