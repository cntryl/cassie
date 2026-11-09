//! Deferred source-phase guarded chunk probes.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

#[test]
fn should_hold_input_chunk_lease_through_both_distinct_paths_and_flatten() {
    // Arrange
    for scalar in [false, true] {
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
        let controls = if scalar {
            controls.for_relational_scalar_cte()
        } else {
            controls
        };
        let parsed =
            crate::sql::parse_statement("SELECT json_object_keys(doc) AS keys FROM records")
                .expect("SQL");
        let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
            panic!("SELECT")
        };
        let values = serde_json::json!(["a", "a", "b"]);
        let batches = vec![vec![BatchRow::new(vec![(
            "keys".into(),
            Value::Json(values),
        )])]];
        let mut guarded =
            projection::expand_json_object_keys(batches, &select.projection, &controls)
                .expect("expand");
        let chunk_bytes = guarded.chunk_bytes();
        // Act
        guarded
            .transform(|batches| distinct_batches(batches, &controls))
            .expect("distinct");
        // Assert
        assert!(chunk_bytes > 0);
        assert_eq!(guarded.chunk_bytes(), chunk_bytes);
        let rows = guarded.flatten(&controls).expect("flatten");
        assert_eq!(rows.len(), 2);
        drop(rows);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_flatten_untracked_exists_or_unselected_batches_without_chunk_charge() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let guarded = projection::OwnedExpanded::untracked(vec![vec![BatchRow::new(vec![(
        "v".into(),
        Value::Int64(1),
    )])]]);
    // Act
    let rows = guarded.flatten(&controls).expect("flatten");
    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
