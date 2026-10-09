//! Static inspection scratch has its own pre-allocation admission and cancellation boundary.
use super::*;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("inspection cleanup: {error}");
            } else {
                panic!("strict inspection cleanup: {error}");
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
fn inspect(
    cassie: &crate::app::Cassie,
    statement: &ParsedStatement,
    fields: &HashSet<String>,
    controls: &QueryExecutionControls,
) -> Result<bool, QueryError> {
    let ctes = super::super::super::CteContext::new();
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
    references(&context, statement, fields)
}
#[test]
fn should_admit_ancestor_inspection_before_allocating_names() {
    // Arrange
    let directory = Directory(
        std::env::temp_dir().join(format!("cassie-ancestor-inspect-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir(&directory.0).expect("owned directory");
    let cassie = crate::app::Cassie::new_with_data_dir(&directory.0).expect("fixture");
    let statement = crate::sql::parse_statement(
        "SELECT 1 FROM middle m WHERE EXISTS(SELECT 1 FROM inner_rows i WHERE i.id=outer_rows.id)",
    )
    .expect("nested scope");
    let fields = HashSet::from(["outer_rows.id".to_string()]);
    let broad = QueryExecutionControls::from_limits(&limits(4 * 1024 * 1024), Instant::now());
    // Act
    let actual = inspect(&cassie, &statement, &fields, &broad).expect("admitted inspection");
    let peak = broad.peak_query_memory_bytes();
    // Assert
    assert!(actual);
    assert!(peak > 0);
    assert_eq!(broad.current_query_memory_bytes(), 0);
    let tight = QueryExecutionControls::from_limits(&limits(peak - 1), Instant::now());
    assert!(matches!(
        inspect(&cassie, &statement, &fields, &tight),
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert_eq!(tight.current_query_memory_bytes(), 0);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let cancelled = QueryExecutionControls::with_cancellation(
        &limits(4 * 1024 * 1024),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        inspect(&cassie, &statement, &fields, &cancelled),
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(cancelled.current_query_memory_bytes(), 0);
}
