use std::path::PathBuf;
use std::time::Instant;

use crate::config::CassieRuntimeLimits;
use crate::runtime::{ExecutionMode, QueryExecutionControls};

use super::Cassie;

struct Fixture {
    cassie: Option<Cassie>,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cassie-result-key-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie fixture");
        Self {
            cassie: Some(cassie),
            path,
        }
    }

    fn cassie(&self) -> &Cassie {
        self.cassie.as_ref().expect("live fixture")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.cassie.take());
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path).expect("retired store cleanup");
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

#[test]
fn should_retain_transient_result_cache_key_admission_until_context_retirement() {
    // Arrange
    let fixture = Fixture::new();
    let cassie = fixture.cassie();
    let session = cassie.create_session("tester", Some("key_database".to_owned()));
    session.set_search_path(vec!["public".to_owned(), "key_schema".repeat(32)]);
    let parsed = crate::sql::parser::parse_statement("SELECT 1").expect("parsed SQL");
    let controls = controls(64 * 1024);

    // Act
    let context = cassie
        .query_cache_context_with_controls(
            &session,
            &parsed,
            crate::runtime::sql_fingerprint(&parsed),
            &[],
            (ExecutionMode::SimpleQuery, &[]),
            &controls,
        )
        .expect("baseline context construction");
    let live = controls.current_query_memory_bytes();
    let preserved = context.exec_cache_key.as_ref().is_some_and(|key| {
        key.user == session.user
            && key.database == session.database
            && key.search_path == session.search_path()
    });
    drop(context);
    let released = controls.current_query_memory_bytes();
    let peak = controls.peak_query_memory_bytes();
    drop(fixture);

    // Assert
    assert!(preserved);
    assert!(
        live > 0,
        "the nested transient lookup key must be admitted while owned"
    );
    assert!(peak >= live);
    assert_eq!(released, 0);
}

#[test]
fn should_bypass_optional_result_cache_key_given_no_memory() {
    // Arrange
    let fixture = Fixture::new();
    let cassie = fixture.cassie();
    let session = cassie.create_session("tester", None);
    let parsed = crate::sql::parser::parse_statement("SELECT 1").expect("parsed SQL");
    let controls = controls(0);
    let before = cassie
        .runtime
        .snapshot()
        .execution_result_cache
        .bypass_reasons
        .get("memory_budget")
        .copied()
        .unwrap_or_default();

    // Act
    let context = cassie
        .query_cache_context_with_controls(
            &session,
            &parsed,
            crate::runtime::sql_fingerprint(&parsed),
            &[],
            (ExecutionMode::SimpleQuery, &[]),
            &controls,
        )
        .expect("baseline context construction");
    let bypassed = context.exec_cache_key.is_none();
    let eligible = context.is_select && context.cache_key.is_some();
    let after = cassie
        .runtime
        .snapshot()
        .execution_result_cache
        .bypass_reasons
        .get("memory_budget")
        .copied()
        .unwrap_or_default();
    drop(context);
    let released = controls.current_query_memory_bytes();
    drop(fixture);

    // Assert
    assert!(
        bypassed,
        "an optional key cannot allocate against an exhausted query budget"
    );
    assert!(
        eligible,
        "result-cache denial must preserve query and planner eligibility"
    );
    assert_eq!(after, before + 1);
    assert_eq!(released, 0);
}
