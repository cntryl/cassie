//! Joined WHERE outer-copy admission and literal alias ownership.
use super::*;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{DataType, Value};
use std::sync::Arc;
use std::time::Instant;

struct Directory(std::path::PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("joined copy cleanup during unwind: {error}");
            } else {
                panic!("joined copy strict cleanup: {error}");
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

fn input(
    controls: &QueryExecutionControls,
    left: &str,
    right: &str,
) -> (BatchRow, Arc<QueryMemoryReservation>) {
    let row = BatchRow::with_aliases(
        vec![
            ("a.b".into(), Value::Int64(10)),
            ("A.B".into(), Value::Int64(20)),
        ],
        vec![(format!("{left}.a.b"), 0), (format!("{right}.A.B"), 1)],
    )
    .with_optional_data_types(Some(Arc::new(vec![DataType::BigInt, DataType::BigInt])));
    let parent = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("source body"))
            .expect("source admission"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&parent)))
        .retain_operator_memory(controls, Arc::clone(&parent))
        .expect("source owner");
    (row, parent)
}

fn copy(
    cassie: &super::super::Cassie,
    source: &QuerySource,
    row: &BatchRow,
    controls: &QueryExecutionControls,
) -> Result<BatchRow, QueryError> {
    let ctes = super::super::CteContext::new();
    let functions = std::collections::HashMap::new();
    let context = ExistsResolutionContext {
        cassie,
        session: None,
        cte_context: &ctes,
        user_functions: &functions,
        params: &[],
        controls,
        outer_row: None,
    };
    qualified_outer_row(&context, source, row, None)
}

#[test]
fn should_preserve_joined_where_literal_copy_ownership() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-joined-copy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path).expect("private directory");
    let directory = Directory(path.clone());
    let cassie = super::super::Cassie::new_with_data_dir(&path).expect("fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE joined_copy (\"a.b\" BIGINT,\"A.B\" BIGINT)",
            vec![],
        )
        .expect("declared literal provenance");
    let statement = crate::sql::parse_statement(
        "SELECT l.\"a.b\",r.\"A.B\" FROM joined_copy l JOIN joined_copy r ON true",
    )
    .expect("joined source SQL");
    let logical = super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
        .expect("joined source plan");
    let source = logical.source;
    let (left, right) = if let QuerySource::Join { left, right, .. } = &source {
        (
            outer_qualifier(left).expect("bound left qualifier"),
            outer_qualifier(right).expect("bound right qualifier"),
        )
    } else {
        panic!("joined source required");
    };
    let left_literal = format!("{left}.\"a.b\"");
    let right_literal = format!("{right}.\"A.B\"");
    let generous = QueryExecutionControls::from_limits(&limits(4 * 1024 * 1024), Instant::now());
    let (row, parent) = input(&generous, &left, &right);
    let before = generous.current_query_memory_bytes();

    // Act
    let retained = copy(&cassie, &source, &row, &generous).expect("joined admitted copy");
    let peak = generous.peak_query_memory_bytes();

    // Assert
    assert!(generous.current_query_memory_bytes() > before);
    assert_eq!(retained.get(&left_literal), Some(&Value::Int64(10)));
    assert_eq!(retained.get(&right_literal), Some(&Value::Int64(20)));
    assert!(retained
        .aliases()
        .iter()
        .any(|(name, index)| name == &right_literal && *index == 1));
    assert_eq!(
        retained.shared_data_types().expect("types").as_ref(),
        &[DataType::BigInt, DataType::BigInt]
    );
    let weak = Arc::downgrade(&parent);
    drop(row);
    drop(parent);
    assert!(weak.upgrade().is_some());
    drop(retained);
    assert!(weak.upgrade().is_none());
    assert_eq!(generous.current_query_memory_bytes(), 0);

    let tight = QueryExecutionControls::from_limits(&limits(peak - 1), Instant::now());
    let (row, parent) = input(&tight, &left, &right);
    let original = tight.current_query_memory_bytes();
    let denied = copy(&cassie, &source, &row, &tight);
    assert!(matches!(
        denied,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert_eq!(tight.current_query_memory_bytes(), original);
    assert_eq!(row.get(&format!("{right}.A.B")), Some(&Value::Int64(20)));
    drop(row);
    drop(parent);
    assert_eq!(tight.current_query_memory_bytes(), 0);

    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let cancelled = QueryExecutionControls::with_cancellation(
        &limits(4 * 1024 * 1024),
        Instant::now(),
        cancellation.clone(),
    );
    let (row, parent) = input(&cancelled, &left, &right);
    let original = cancelled.current_query_memory_bytes();
    cancellation.cancel();
    let result = copy(&cassie, &source, &row, &cancelled);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(cancelled.current_query_memory_bytes(), original);
    drop(row);
    drop(parent);
    assert_eq!(cancelled.current_query_memory_bytes(), 0);
    println!("joined WHERE copy peak={peak} denied_budget={}", peak - 1);
    drop(session);
    drop(cassie);
    drop(directory);
    assert!(!path.exists());
}
