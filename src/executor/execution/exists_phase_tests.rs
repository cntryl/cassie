//! Actual scalar phase invocation admits its carrier while retaining ancestor owners.
use super::{
    BatchRow, Cassie, CteContext, Expr, QueryError, QueryExecutionControls, QuerySource, Value,
};
use crate::executor::batch::RowAccess;
use crate::runtime::QueryMemoryReservation;
use crate::types::DataType;
use std::sync::Arc;
use std::time::Instant;

struct Fixture {
    cassie: Cassie,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("cassie-phase-admission-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("setup", None);
        for sql in [
            "CREATE TABLE phase_member (id BIGINT)",
            "INSERT INTO phase_member VALUES (1)",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("phase fixture");
        }
        Self { cassie, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.cassie.shutdown();
    }
}
fn limits(budget: usize) -> crate::config::CassieRuntimeLimits {
    crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        ..crate::config::CassieRuntimeLimits::default()
    }
}
fn input(controls: &QueryExecutionControls) -> (BatchRow, Arc<QueryMemoryReservation>) {
    let ancestor = BatchRow::new(vec![
        ("ancestor.id".into(), Value::Int64(1)),
        ("ancestor.payload".into(), Value::String("x".repeat(768))),
    ])
    .with_optional_data_types(Some(Arc::new(vec![DataType::BigInt, DataType::Text])));
    let parent = Arc::new(
        controls
            .reserve_query_memory(
                ancestor.unleased_body_bytes().expect("ancestor backing")
                    + std::mem::size_of::<BatchRow>()
                    + 2 * std::mem::size_of::<usize>(),
            )
            .expect("ancestor admission"),
    );
    let ancestor = ancestor
        .with_query_memory(Some(Arc::clone(&parent)))
        .retain_operator_memory(controls, Arc::clone(&parent))
        .expect("ancestor owner");
    let row = BatchRow::new(vec![
        ("current.id".into(), Value::Int64(1)),
        (
            "payload".into(),
            Value::Json(serde_json::json!(["y".repeat(513), null])),
        ),
    ])
    .with_optional_data_types(Some(Arc::new(vec![
        DataType::BigInt,
        DataType::Array(Box::new(DataType::Text)),
    ])));
    let memory = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("row backing"))
            .expect("row admission"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&memory)))
        .retain_operator_memory(controls, memory)
        .expect("row owner")
        .with_outer_scope(Arc::new(ancestor));
    (row, parent)
}
fn expression() -> Expr {
    let parsed = crate::sql::parse_statement(
        "SELECT EXISTS(SELECT 1 FROM phase_member i WHERE i.id=ancestor.id AND i.id=current.id)",
    )
    .expect("phase expression");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let crate::sql::ast::SelectItem::Expr { expr, .. } =
        select.projection.into_iter().next().expect("projection")
    else {
        panic!("expression");
    };
    expr
}
fn evaluate(
    fixture: &Fixture,
    row: &BatchRow,
    controls: &QueryExecutionControls,
) -> Result<Value, QueryError> {
    let ctes = CteContext::new();
    let functions = std::collections::HashMap::new();
    let env = super::source::source_execution_env(&fixture.cassie, None, &functions, &[], controls);
    super::exists_phase::evaluate(
        &env,
        &ctes,
        &QuerySource::SingleRow,
        row,
        &expression(),
        None,
    )
}
fn cleanup(fixture: Fixture) {
    let path = fixture.path.clone();
    drop(fixture);
    std::fs::remove_dir_all(path).expect("strict phase admission cleanup");
}

#[test]
fn should_preserve_actual_phase_carrier_owners_during_evaluation() {
    // Arrange
    let fixture = Fixture::new();
    let controls = QueryExecutionControls::from_limits(&limits(4 * 1024 * 1024), Instant::now());
    let (row, parent) = input(&controls);
    let original = controls.current_query_memory_bytes();
    let ancestor_owner = Arc::downgrade(&parent);
    drop(parent);
    assert!(ancestor_owner.upgrade().is_some());
    // Act
    let actual = evaluate(&fixture, &row, &controls);
    // Assert
    assert_eq!(
        actual.expect("actual borrowed phase evaluation"),
        Value::Bool(true)
    );
    assert_eq!(controls.current_query_memory_bytes(), original);
    assert!(ancestor_owner.upgrade().is_some());
    assert_eq!(
        row.column_type("payload"),
        Some(&DataType::Array(Box::new(DataType::Text)))
    );
    assert!(controls.peak_query_memory_bytes() > original);
    assert_eq!(row.get("ancestor.id"), Some(&Value::Int64(1)));
    assert_eq!(
        row.outer_scope().expect("ancestor").data_types(),
        &[DataType::BigInt, DataType::Text]
    );
    drop(row);
    assert!(ancestor_owner.upgrade().is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
    cleanup(fixture);
}

#[test]
fn should_preserve_input_owners_after_phase_carrier_rejection() {
    // Arrange
    let fixture = Fixture::new();
    let broad = QueryExecutionControls::from_limits(&limits(4 * 1024 * 1024), Instant::now());
    let (row, parent) = input(&broad);
    evaluate(&fixture, &row, &broad).expect("measure real phase peak");
    let peak = broad.peak_query_memory_bytes();
    drop(row);
    drop(parent);
    assert_eq!(broad.current_query_memory_bytes(), 0);
    let tight = QueryExecutionControls::from_limits(&limits(peak - 1), Instant::now());
    let (row, parent) = input(&tight);
    let original = tight.current_query_memory_bytes();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let cancelled = QueryExecutionControls::with_cancellation(
        &limits(4 * 1024 * 1024),
        Instant::now(),
        cancellation.clone(),
    );
    let (cancelled_row, cancelled_parent) = input(&cancelled);
    let cancelled_original = cancelled.current_query_memory_bytes();
    cancellation.cancel();
    // Act
    let denied = evaluate(&fixture, &row, &tight);
    let interrupted = evaluate(&fixture, &cancelled_row, &cancelled);
    // Assert
    assert!(
        matches!(
            denied,
            Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                _
            )))
        ),
        "{denied:?}"
    );
    assert!(
        matches!(
            interrupted,
            Err(QueryError::General(ref message)) if message == "query canceled"
        ),
        "{interrupted:?}"
    );
    assert_eq!(tight.current_query_memory_bytes(), original);
    assert_eq!(cancelled.current_query_memory_bytes(), cancelled_original);
    assert_eq!(row.get("ancestor.id"), Some(&Value::Int64(1)));
    drop(row);
    drop(parent);
    drop(cancelled_row);
    drop(cancelled_parent);
    assert_eq!(tight.current_query_memory_bytes(), 0);
    assert_eq!(cancelled.current_query_memory_bytes(), 0);
    cleanup(fixture);
}
