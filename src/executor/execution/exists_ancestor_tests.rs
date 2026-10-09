//! Ancestor references retain existing Arc backing at each copied scalar scope.
use super::super::{Cassie, CteContext};
use super::{qualified_outer_row, scoped_outer_row};
use super::{BatchRow, ExistsResolutionContext, LogicalPlan, QueryError};
use crate::executor::batch::RowAccess;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{DataType, Value};
use std::sync::Arc;
use std::time::Instant;

struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("ancestor cleanup: {error}");
            } else {
                panic!("strict ancestor cleanup: {error}");
            }
        }
    }
}
fn limits(budget: usize) -> crate::config::CassieRuntimeLimits {
    crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..crate::config::CassieRuntimeLimits::default()
    }
}
fn input(controls: &QueryExecutionControls) -> (BatchRow, Arc<QueryMemoryReservation>) {
    let ancestor = BatchRow::new(vec![
        ("o.id".into(), Value::Int64(1)),
        (
            "o.a".into(),
            Value::Json(serde_json::json!(["x".repeat(768), null])),
        ),
        ("o.s".into(), Value::String("y".repeat(513))),
    ])
    .with_optional_data_types(Some(Arc::new(vec![
        DataType::BigInt,
        DataType::Array(Box::new(DataType::Text)),
        DataType::Text,
    ])));
    let parent = Arc::new(
        controls
            .reserve_query_memory(
                ancestor.unleased_body_bytes().expect("ancestor body")
                    + std::mem::size_of::<BatchRow>()
                    + 2 * std::mem::size_of::<usize>(),
            )
            .expect("ancestor admission"),
    );
    let ancestor = ancestor
        .with_query_memory(Some(Arc::clone(&parent)))
        .retain_operator_memory(controls, Arc::clone(&parent))
        .expect("ancestor owner");
    let row = BatchRow::new(vec![("id".into(), Value::Int64(10))])
        .with_optional_data_types(Some(Arc::new(vec![DataType::BigInt])));
    let memory = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("middle body"))
            .expect("middle admission"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&memory)))
        .retain_operator_memory(controls, memory)
        .expect("middle owner")
        .with_outer_scope(Arc::new(ancestor));
    (row, parent)
}
fn copy(
    cassie: &Cassie,
    plan: &LogicalPlan,
    row: &BatchRow,
    controls: &QueryExecutionControls,
    where_copy: bool,
) -> Result<BatchRow, QueryError> {
    let ctes = CteContext::new();
    let functions = std::collections::HashMap::new();
    let context = ExistsResolutionContext {
        cassie,
        session: None,
        cte_context: &ctes,
        user_functions: &functions,
        params: &[],
        controls,
        outer_row: Some(row),
    };
    if where_copy {
        qualified_outer_row(&context, &crate::sql::QuerySource::SingleRow, row, None)
    } else {
        scoped_outer_row(&context, plan, row)
    }
}
#[test]
fn should_preserve_ancestor_copy_ownership() {
    // Arrange
    let directory = Directory(
        std::env::temp_dir().join(format!("cassie-ancestor-owner-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir(&directory.0).expect("owned directory");
    let cassie = Cassie::new_with_data_dir(&directory.0).expect("fixture");
    let statement =
        crate::sql::parse_statement("SELECT CAST(1 AS BIGINT) AS n").expect("inner SQL");
    let plan =
        super::super::build_logical_plan_in_session(&cassie, None, &statement).expect("inner plan");
    for where_copy in [false, true] {
        let broad = QueryExecutionControls::from_limits(&limits(4 * 1024 * 1024), Instant::now());
        let (row, parent) = input(&broad);
        let before = broad.current_query_memory_bytes();
        // Act
        let retained =
            copy(&cassie, &plan, &row, &broad, where_copy).expect("admitted ancestor carrier");
        let peak = broad.peak_query_memory_bytes();
        // Assert
        assert!(broad.current_query_memory_bytes() > before);
        assert_eq!(retained.get("o.id"), Some(&Value::Int64(1)));
        assert_eq!(retained.get("id"), Some(&Value::Int64(10)));
        assert_eq!(
            retained.column_type("o.a"),
            Some(&DataType::Array(Box::new(DataType::Text)))
        );
        assert_eq!(retained.get("o.s"), Some(&Value::String("y".repeat(513))));
        let weak = Arc::downgrade(&parent);
        drop(row);
        drop(parent);
        assert!(weak.upgrade().is_some());
        drop(retained);
        assert!(weak.upgrade().is_none());
        assert_eq!(broad.current_query_memory_bytes(), 0);
        let tight = QueryExecutionControls::from_limits(&limits(peak - 1), Instant::now());
        let (row, parent) = input(&tight);
        let original = tight.current_query_memory_bytes();
        let result = copy(&cassie, &plan, &row, &tight, where_copy);
        assert!(
            matches!(
                result,
                Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                    _
                )))
            ),
            "{result:?}"
        );
        assert_eq!(tight.current_query_memory_bytes(), original);
        assert_eq!(row.get("o.id"), Some(&Value::Int64(1)));
        drop(row);
        drop(parent);
        assert_eq!(tight.current_query_memory_bytes(), 0);
        let cancellation = crate::runtime::QueryCancellationHandle::new();
        let cancelled = QueryExecutionControls::with_cancellation(
            &limits(4 * 1024 * 1024),
            Instant::now(),
            cancellation.clone(),
        );
        let (row, parent) = input(&cancelled);
        let original = cancelled.current_query_memory_bytes();
        cancellation.cancel();
        let result = copy(&cassie, &plan, &row, &cancelled, where_copy);
        if where_copy {
            assert!(
                matches!(
                    result,
                    Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
                ),
                "{result:?}"
            );
        } else {
            assert!(
                matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"),
                "{result:?}"
            );
        }
        assert_eq!(cancelled.current_query_memory_bytes(), original);
        drop(row);
        drop(parent);
        assert_eq!(cancelled.current_query_memory_bytes(), 0);
    }
}
