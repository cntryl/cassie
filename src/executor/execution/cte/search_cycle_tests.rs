use super::{append_recursive_metadata, encode_lineage, lineage_column_name, Lineage};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::ast::{CteCycle, CteSearch, CteSearchOrder, Expr};
use crate::types::Value;
use std::time::Instant;

#[test]
fn should_stop_recursive_search_cycle_metadata_when_cancelled() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let lineage = encode_lineage(&Lineage::default()).expect("encode lineage");
    let input = vec![vec![
        ("node".to_owned(), Value::Int64(1)),
        (lineage_column_name("walk"), lineage),
    ]];
    let mut rows = super::super::RetainedRows::copy(&input, &controls).expect("retained rows");
    let original_rows = rows.rows.clone();
    let search = CteSearch {
        order: CteSearchOrder::DepthFirst,
        by: vec!["node".to_owned()],
        sequence_column: "sequence".to_owned(),
    };
    let cycle = CteCycle {
        by: vec!["node".to_owned()],
        mark_column: "is_cycle".to_owned(),
        mark_value: Expr::BoolLiteral(true),
        default_value: Expr::BoolLiteral(false),
        path_column: "path".to_owned(),
    };
    cancellation.cancel();

    // Act
    let result =
        append_recursive_metadata(&mut rows, "walk", Some(&search), Some(&cycle), &controls);

    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(rows.rows, original_rows);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
